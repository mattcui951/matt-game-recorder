//! The recorder: one ffmpeg process capturing a window or monitor on the GPU (Windows Graphics
//! Capture -> hardware encoder), with audio fed through a named pipe. ffmpeg writes fragmented
//! MP4 to stdout; we split it into keyframe-aligned fragments and route them to:
//!   * a file (full match / full session), and/or
//!   * a ring buffer in RAM or on disk (highlights / manual clips), from which clips are cut
//!     by copying fragments, never re-encoding.

pub mod audio;
pub mod fmp4;

use crate::ffmpeg;
use crate::paths;
use crate::win::hidden_command;
use anyhow::{bail, Context, Result};
use audio::{AudioConfig, AudioPipeline};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum Target {
    Window(isize),
    Monitor(isize),
}

#[derive(Clone, Debug)]
pub struct RingOptions {
    /// Keep fragments in memory (true) or in a temp file (false).
    pub in_ram: bool,
    pub temp_dir: PathBuf,
    /// Seconds of history always kept.
    pub keep_secs: f64,
}

#[derive(Clone, Debug)]
pub struct CaptureOptions {
    pub target: Target,
    pub fps: u32,
    /// Output frame size; None keeps the source size.
    pub size: Option<(u32, u32)>,
    /// When `size` is set: stretch (true) or fit with bars (false).
    pub stretch: bool,
    pub cursor: bool,
    pub border: bool,
    pub encoder: String,
    pub encoder_preset: String,
    pub bitrate_kbps: u32,
    pub audio: AudioConfig,
    pub webcam: Option<String>,
    /// Write the whole recording here (fragmented while recording, finalized on stop).
    pub file: Option<PathBuf>,
    pub ring: Option<RingOptions>,
}

enum RingData {
    Mem(Arc<Vec<u8>>),
    Disk { offset: u64, len: usize },
}

struct RingEntry {
    start: f64,
    data: RingData,
}

#[derive(Default)]
struct State {
    init: Vec<u8>,
    prev_start: f64,
    last_start: f64,
    last_arrival: Option<Instant>,
    fragments: u64,
    file: Option<File>,
    ring: VecDeque<RingEntry>,
    ring_disk: Option<(File, PathBuf, u64)>,
    keep_secs: f64,
    pins: Vec<(u64, f64)>,
    next_pin: u64,
    error: Option<String>,
}

pub struct Recorder {
    child: Child,
    stdin: Option<ChildStdin>,
    audio: Option<AudioPipeline>,
    state: Arc<Mutex<State>>,
    reader: Option<JoinHandle<()>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    part_path: Option<PathBuf>,
    final_path: Option<PathBuf>,
    pub started: Instant,
}

pub struct Finished {
    pub file: Option<PathBuf>,
    pub duration: f64,
}

/// Delay between a frame being captured and its fragment reaching us, beyond one fragment
/// length (encoder lookahead, filter and mux queues). Measured on NVENC AV1 against an on-screen
/// wall clock with tools/mock_lol.py.
const CAPTURE_LAG: f64 = 2.5;

impl Recorder {
    pub fn start(opts: CaptureOptions) -> Result<Recorder> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let audio = AudioPipeline::start(opts.audio.clone(), &id[..12])?;

        let mut src = match opts.target {
            Target::Window(h) => format!("gfxcapture=hwnd={h}"),
            Target::Monitor(h) => format!("gfxcapture=hmonitor={h}"),
        };
        src.push_str(&format!(
            ":max_framerate={fps}:capture_cursor={c}:display_border={b}",
            fps = opts.fps,
            c = opts.cursor as u8,
            b = opts.border as u8
        ));
        if let Some((w, h)) = opts.size {
            src.push_str(&format!(":width={}:height={}:resize_mode={}:scale_mode=bicubic", w / 2 * 2, h / 2 * 2, if opts.stretch { "scale" } else { "scale_aspect" }));
        }
        let encoder = ffmpeg::best_encoder(&opts.encoder);
        let graph = match &opts.webcam {
            Some(_) => {
                // Webcam in the bottom-right corner at a quarter of the frame height.
                let frame_h = opts.size.map(|s| s.1).unwrap_or(1080);
                let cam_h = (frame_h / 4 / 2 * 2).max(120);
                format!(
                    "{src},fps={fps},hwdownload,format=bgra[g];[1:v]scale=-2:{cam_h}[cam];\
                     [g][cam]overlay@cam=x=W-w-24:y=H-h-24,format=nv12[v]",
                    fps = opts.fps
                )
            }
            None => format!("{src},fps={fps}{up}[v]", fps = opts.fps, up = ffmpeg::capture_upload_chain(&encoder)),
        };

        let mut cmd = hidden_command(paths::ffmpeg());
        cmd.args(["-hide_banner", "-loglevel", "warning", "-nostats"]);
        cmd.args(["-f", "f32le", "-ar", "48000", "-ac", "2", "-thread_queue_size", "4096", "-i", audio.pipe_name()]);
        if let Some(cam) = &opts.webcam {
            cmd.args(["-f", "dshow", "-rtbufsize", "64M", "-thread_queue_size", "512", "-i"]).arg(format!("video={cam}"));
        }
        cmd.args(["-filter_complex", &graph, "-map", "[v]", "-map", "0:a"]);
        cmd.args(ffmpeg::video_encoder_args(&encoder, &opts.encoder_preset, opts.bitrate_kbps, opts.fps));
        cmd.args(ffmpeg::audio_encoder_args());
        // flush_packets: hand each fragment over the pipe as soon as it is complete, otherwise its
        // tail waits in ffmpeg's I/O buffer until the next fragment and the clock runs late.
        cmd.args(["-flush_packets", "1", "-f", "mp4", "-movflags", "frag_keyframe+empty_moov+default_base_moof", "pipe:1"]);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        log::info!("starting capture with {encoder}: {graph}");
        let mut child = cmd.spawn().context(tr!("failed to start ffmpeg", "无法启动 ffmpeg"))?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
        {
            let tail = stderr_tail.clone();
            std::thread::Builder::new().name("ffmpeg-stderr".into()).spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
                    log::debug!("ffmpeg: {line}");
                    let mut t = tail.lock();
                    if t.len() >= 30 {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            })?;
        }

        let part_path = opts.file.as_ref().map(|f| f.with_extension("part.mp4"));
        let mut state = State { keep_secs: 0.0, ..Default::default() };
        if let Some(p) = &part_path {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir)?;
            }
            state.file = Some(File::create(p).with_context(|| format!("cannot create {}", p.display()))?);
        }
        if let Some(r) = &opts.ring {
            state.keep_secs = r.keep_secs;
            if !r.in_ram {
                std::fs::create_dir_all(&r.temp_dir)?;
                let path = r.temp_dir.join(format!("buffer-{id}.tmp"));
                state.ring_disk = Some((File::options().create(true).read(true).write(true).truncate(true).open(&path)?, path, 0));
            }
        }
        let has_ring = opts.ring.is_some();
        let state = Arc::new(Mutex::new(state));
        let st = state.clone();
        let reader = std::thread::Builder::new().name("capture-reader".into()).spawn(move || {
            let mut r = fmp4::Fmp4Reader::new(BufReader::with_capacity(1 << 20, stdout));
            loop {
                match r.next_fragment() {
                    Ok(Some(frag)) => {
                        let mut s = st.lock();
                        if s.init.is_empty() {
                            s.init = r.init.clone();
                            let init = s.init.clone();
                            if let Some(f) = s.file.as_mut() {
                                let _ = f.write_all(&init);
                            }
                        }
                        if let Err(e) = s.file.as_mut().map_or(Ok(()), |f| f.write_all(&frag.bytes)) {
                            s.error = Some(format!("write failed: {e}"));
                        }
                        if has_ring {
                            push_ring(&mut s, frag.start, frag.bytes);
                        }
                        s.prev_start = s.last_start;
                        s.last_start = frag.start;
                        s.last_arrival = Some(Instant::now());
                        s.fragments += 1;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        st.lock().error = Some(format!("stream error: {e}"));
                        break;
                    }
                }
            }
            if let Some(f) = st.lock().file.as_mut() {
                let _ = f.flush();
            }
        })?;

        Ok(Recorder {
            child,
            stdin,
            audio: Some(audio),
            state,
            reader: Some(reader),
            stderr_tail,
            part_path,
            final_path: opts.file,
            started: Instant::now(),
        })
    }

    /// Current position in the recording, in seconds of video.
    pub fn now(&self) -> f64 {
        let s = self.state.lock();
        match s.last_arrival {
            // A fragment arrives one fragment length after its start plus the encode/mux pipeline
            // delay.
            Some(at) => s.last_start + s.frag_len() + CAPTURE_LAG + at.elapsed().as_secs_f64(),
            None => 0.0,
        }
    }

    pub fn is_producing(&self) -> bool {
        self.state.lock().fragments > 0
    }

    /// Error text if ffmpeg died or the stream broke.
    pub fn failure(&mut self) -> Option<String> {
        if let Some(e) = self.state.lock().error.clone() {
            return Some(e);
        }
        match self.child.try_wait() {
            Ok(Some(status)) => Some(format!("ffmpeg exited ({status}): {}", self.stderr_tail.lock().iter().cloned().collect::<Vec<_>>().join(" | "))),
            _ => None,
        }
    }

    /// Keep everything from `from` seconds on until `unpin` (used while a manual capture runs).
    pub fn pin(&self, from: f64) -> u64 {
        let mut s = self.state.lock();
        s.next_pin += 1;
        let id = s.next_pin;
        s.pins.push((id, from));
        id
    }

    pub fn unpin(&self, id: u64) {
        self.state.lock().pins.retain(|(p, _)| *p != id);
    }

    /// Copy `[from, to]` out of the ring buffer into a finished mp4 at `output`.
    /// Returns the actual (start, end) covered, which are fragment-aligned.
    pub fn extract(&self, from: f64, to: f64, output: &Path) -> Result<(f64, f64)> {
        let tmp = output.with_extension("cut.mp4");
        let (start, end) = {
            let mut s = self.state.lock();
            if s.init.is_empty() || s.ring.is_empty() {
                bail!(tr!("Nothing is buffered yet. Try again in a moment.", "还没有缓存内容，请稍后再试。"));
            }
            let n = s.ring.len();
            let mut picked: Vec<usize> = Vec::new();
            for i in 0..n {
                let st = s.ring[i].start;
                let en = if i + 1 < n { s.ring[i + 1].start } else { s.last_start + s.frag_len() };
                if st < to && en > from {
                    picked.push(i);
                }
            }
            if picked.is_empty() {
                bail!(tr!("That part is no longer buffered.", "那一段已不在缓存中。"));
            }
            let first = picked[0];
            let last = *picked.last().unwrap();
            let start = s.ring[first].start;
            let end = if last + 1 < n { s.ring[last + 1].start } else { s.last_start + s.frag_len() };
            if let Some(dir) = tmp.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut out = File::create(&tmp)?;
            out.write_all(&s.init)?;
            for i in picked {
                match &s.ring[i].data {
                    RingData::Mem(b) => out.write_all(b)?,
                    RingData::Disk { offset, len } => {
                        let (offset, len) = (*offset, *len);
                        let (f, _, _) = s.ring_disk.as_mut().context("disk buffer missing")?;
                        let mut buf = vec![0u8; len];
                        f.seek(SeekFrom::Start(offset))?;
                        f.read_exact(&mut buf)?;
                        f.seek(SeekFrom::End(0))?;
                        out.write_all(&buf)?;
                    }
                }
            }
            (start, end)
        };
        let result = ffmpeg::remux_faststart(&tmp, output);
        let _ = std::fs::remove_file(&tmp);
        result?;
        Ok((start, end))
    }

    /// Stop gracefully and finalize the recording file.
    pub fn stop(mut self) -> Result<Finished> {
        let duration = self.now();
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.write_all(b"q");
            let _ = stdin.flush();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                _ if Instant::now() > deadline => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        if let Some(a) = self.audio.take() {
            a.stop();
        }
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        let (file_ok, disk_tmp) = {
            let mut s = self.state.lock();
            s.file = None;
            (s.fragments > 0, s.ring_disk.take().map(|(_, p, _)| p))
        };
        if let Some(p) = disk_tmp {
            let _ = std::fs::remove_file(p);
        }
        let mut file = None;
        if let (Some(part), Some(fin)) = (self.part_path.take(), self.final_path.take()) {
            if file_ok {
                match ffmpeg::remux_faststart(&part, &fin) {
                    Ok(()) => {
                        let _ = std::fs::remove_file(&part);
                        file = Some(fin);
                    }
                    Err(e) => {
                        // Keep the fragmented file; it is still playable.
                        log::error!("finalize failed, keeping fragmented file: {e:#}");
                        let _ = std::fs::rename(&part, &fin);
                        file = Some(fin);
                    }
                }
            } else {
                let _ = std::fs::remove_file(&part);
            }
        }
        let duration = file.as_ref().and_then(|f| ffmpeg::probe(f).ok()).map(|p| p.duration).filter(|d| *d > 0.0).unwrap_or(duration);
        Ok(Finished { file, duration })
    }

    /// Show or hide the webcam overlay live (moves it off-frame via an ffmpeg filter command).
    pub fn set_webcam_visible(&mut self, visible: bool) {
        if let Some(stdin) = self.stdin.as_mut() {
            let x = if visible { "W-w-24" } else { "W+10" };
            let _ = stdin.write_all(format!("coverlay@cam -1 x {x}\n").as_bytes());
            let _ = stdin.flush();
        }
    }

    pub fn last_error_lines(&self) -> String {
        self.stderr_tail.lock().iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if self.reader.is_some() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            if let Some(a) = self.audio.take() {
                a.stop();
            }
        }
    }
}

impl State {
    fn frag_len(&self) -> f64 {
        let d = self.last_start - self.prev_start;
        if d > 0.1 && d < 10.0 { d } else { ffmpeg::KEYFRAME_SECS as f64 }
    }
}

fn push_ring(s: &mut State, start: f64, bytes: Vec<u8>) {
    let data = match s.ring_disk.as_mut() {
        Some((f, _, end)) => {
            let offset = *end;
            if f.write_all(&bytes).is_err() {
                s.error = Some("buffer write failed".into());
                return;
            }
            *end += bytes.len() as u64;
            RingData::Disk { offset, len: bytes.len() }
        }
        None => RingData::Mem(Arc::new(bytes)),
    };
    s.ring.push_back(RingEntry { start, data });
    // Drop history older than the keep window and every pin.
    let oldest_needed = s.pins.iter().map(|(_, t)| *t).fold(start - s.keep_secs, f64::min);
    while s.ring.len() > 1 && s.ring[1].start <= oldest_needed {
        s.ring.pop_front();
    }
}

/// Grab a single frame of a window to an image file.
pub fn screenshot(target: &Target, output: &Path) -> Result<()> {
    let src = match target {
        Target::Window(h) => format!("gfxcapture=hwnd={h}:capture_cursor=0"),
        Target::Monitor(h) => format!("gfxcapture=hmonitor={h}:capture_cursor=0"),
    };
    if let Some(dir) = output.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out = hidden_command(paths::ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-y", "-filter_complex"])
        .arg(format!("{src},hwdownload,format=bgra[v]"))
        .args(["-map", "[v]", "-frames:v", "1"])
        .arg(output)
        .stdin(Stdio::null())
        .output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        bail!(tr!("screenshot failed: {err}", "截图失败：{err}"));
    }
    Ok(())
}

/// Webcam devices DirectShow exposes.
pub fn list_webcams() -> Vec<String> {
    let out = hidden_command(paths::ffmpeg())
        .args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .stdin(Stdio::null())
        .output();
    let Ok(out) = out else { return vec![] };
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|l| l.contains("(video)"))
        .filter_map(|l| l.split('"').nth(1).map(str::to_string))
        .collect()
}
