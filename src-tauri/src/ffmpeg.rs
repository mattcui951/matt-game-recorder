//! FFmpeg/ffprobe helpers: encoder selection, remux, cut, thumbnails, rendering.

use crate::paths;
use crate::win::hidden_command;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderInfo {
    pub id: String,
    pub label: String,
    pub available: bool,
}

const ENCODERS: &[(&str, &str)] = &[
    ("av1_nvenc", "NVIDIA NVENC AV1 (GPU, smallest files)"),
    ("hevc_nvenc", "NVIDIA NVENC HEVC (GPU)"),
    ("h264_nvenc", "NVIDIA NVENC H.264 (GPU, most compatible)"),
    ("av1_amf", "AMD AMF AV1 (GPU)"),
    ("hevc_amf", "AMD AMF HEVC (GPU)"),
    ("h264_amf", "AMD AMF H.264 (GPU)"),
    ("av1_qsv", "Intel Quick Sync AV1 (GPU)"),
    ("hevc_qsv", "Intel Quick Sync HEVC (GPU)"),
    ("h264_qsv", "Intel Quick Sync H.264 (GPU)"),
    ("libx264", "x264 H.264 (CPU)"),
];

/// Which encoders actually work on this machine (one tiny test encode each, cached).
pub fn encoders() -> &'static [EncoderInfo] {
    static CACHE: OnceLock<Vec<EncoderInfo>> = OnceLock::new();
    CACHE.get_or_init(|| {
        ENCODERS
            .iter()
            .map(|(id, label)| {
                let ok = hidden_command(paths::ffmpeg())
                    .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=black:s=320x240:r=30", "-frames:v", "3", "-c:v", id, "-f", "null", "-"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                EncoderInfo { id: id.to_string(), label: label.to_string(), available: ok }
            })
            .collect()
    })
}

pub const KEYFRAME_SECS: u32 = 2;

pub fn best_encoder(wanted: &str) -> String {
    let list = encoders();
    if list.iter().any(|e| e.id == wanted && e.available) {
        return wanted.to_string();
    }
    list.iter().find(|e| e.available).map(|e| e.id.clone()).unwrap_or_else(|| "libx264".into())
}

pub fn family(encoder: &str) -> &'static str {
    if encoder.ends_with("_nvenc") {
        "nvenc"
    } else if encoder.ends_with("_amf") {
        "amf"
    } else if encoder.ends_with("_qsv") {
        "qsv"
    } else {
        "cpu"
    }
}

/// Filter appended after a D3D11 capture source so the encoder can consume its frames.
pub fn capture_upload_chain(encoder: &str) -> &'static str {
    match family(encoder) {
        // NVENC takes D3D11 BGRA textures directly: zero copies, zero CPU conversion.
        "nvenc" => "",
        _ => ",hwdownload,format=bgra,format=nv12",
    }
}

/// Video encoder arguments. `bitrate_kbps` is the peak; quality-targeted VBR keeps
/// the average well below it on calm scenes, which is where most disk is saved.
pub fn video_encoder_args(encoder: &str, preset: &str, bitrate_kbps: u32, fps: u32) -> Vec<String> {
    let b = bitrate_kbps.max(500);
    let mut a: Vec<String> = vec!["-c:v".into(), encoder.into()];
    let s = |v: &str| v.to_string();
    let kind = if encoder == "libsvtav1" { "svt" } else { family(encoder) };
    match kind {
        "svt" => {
            // Matched against av1_nvenc at cq 33 on a 1080p60 League video: same size, VMAF within
            // a point, ~290 fps on a 24-core CPU. Closed GOPs keep every keyframe a clean cut point.
            a.extend([s("-preset"), s("10"), s("-crf"), s("30"), s("-maxrate"), format!("{b}k"), s("-bufsize"), format!("{}k", b * 2), s("-flags"), s("+cgop")]);
        }
        "nvenc" => {
            // B-frames: the third value. The encoder holds frames for them, which on 1080p60 AV1 is
            // about 170 MB of video memory and 125 MB of RAM more than without; without, files
            // are about 20% larger at the same quality. High performance trades disk for that.
            let (p, tune, b_frames) = match preset {
                "quality" => ("p7", "hq", true),
                "performance" => ("p2", "hq", false),
                "lowlatency" => ("p4", "ll", false),
                "perf_lowlatency" => ("p2", "ull", false),
                "quality_lowlatency" => ("p6", "ll", false),
                _ => ("p5", "hq", true),
            };
            // True constant-quality (-b:v 0) capped by maxrate: calm scenes cost little, fights get the bits.
            let cq = if encoder.starts_with("av1") { "33" } else if encoder.starts_with("hevc") { "28" } else { "24" };
            a.extend([s("-preset"), s(p), s("-tune"), s(tune), s("-rc"), s("vbr"), s("-cq"), s(cq)]);
            if !b_frames {
                a.extend([s("-bf"), s("0")]);
            }
            a.extend([s("-b:v"), s("0"), s("-maxrate"), format!("{b}k"), s("-bufsize"), format!("{}k", b * 2)]);
            if !encoder.starts_with("av1") {
                a.extend([s("-spatial-aq"), s("1")]);
            }
        }
        "amf" => {
            let q = match preset { "quality" => "quality", "performance" | "perf_lowlatency" => "speed", _ => "balanced" };
            a.extend([s("-quality"), s(q), s("-rc"), s("vbr_peak"), s("-b:v"), format!("{}k", b * 2 / 3), s("-maxrate"), format!("{b}k")]);
        }
        "qsv" => {
            let p = match preset { "quality" => "slower", "performance" | "perf_lowlatency" => "veryfast", _ => "medium" };
            a.extend([s("-preset"), s(p), s("-b:v"), format!("{}k", b * 2 / 3), s("-maxrate"), format!("{b}k")]);
        }
        _ => {
            let p = match preset { "quality" => "medium", "performance" | "perf_lowlatency" => "superfast", _ => "veryfast" };
            a.extend([s("-preset"), s(p), s("-crf"), s("23"), s("-maxrate"), format!("{b}k"), s("-bufsize"), format!("{}k", b * 2)]);
            if preset.contains("lowlatency") {
                a.extend([s("-tune"), s("zerolatency")]);
            }
        }
    }
    // A keyframe every 2 s (by time, robust to variable frame rate): cuts land within 2 s
    // without paying for frequent intra frames.
    a.extend([s("-g"), (fps.max(1) * KEYFRAME_SECS).to_string(), s("-force_key_frames"), s(&format!("expr:gte(t,n_forced*{KEYFRAME_SECS})"))]);
    if family(encoder) == "nvenc" {
        // NVENC only turns forced keyframes into real IDR frames with this flag.
        a.extend([s("-forced-idr"), s("1")]);
    }
    a
}

pub fn audio_encoder_args() -> Vec<String> {
    ["-c:a", "libopus", "-b:a", "96k", "-ar", "48000"].iter().map(|s| s.to_string()).collect()
}

fn run(args: &[&str]) -> Result<()> {
    run_out(args).map(drop)
}

/// `run`, returning what ffmpeg wrote to stdout.
fn run_out(args: &[&str]) -> Result<String> {
    let out = hidden_command(paths::ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context(crate::i18n::ffmpeg_start_failed())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        bail!(tr!("ffmpeg failed: {err}",
            zh: "ffmpeg 出错：{err}", tw: "ffmpeg 出錯：{err}",
            es: "ffmpeg falló: {err}", fr: "échec de ffmpeg : {err}",
            ja: "ffmpeg でエラーが発生しました: {err}", ko: "ffmpeg 오류: {err}", ru: "ошибка ffmpeg: {err}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn p(path: &Path) -> &str {
    path.to_str().unwrap_or_default()
}

/// Rewrite a (fragmented) mp4 as a regular mp4 with the index up front, without re-encoding.
pub fn remux_faststart(input: &Path, output: &Path) -> Result<()> {
    run(&["-i", p(input), "-map", "0", "-c", "copy", "-movflags", "+faststart", p(output)])
}

/// Cut `[start, end]` without re-encoding (snaps to the keyframe at or before `start`).
pub fn cut_copy(input: &Path, start: f64, end: f64, output: &Path) -> Result<()> {
    let (ss, t) = (format!("{:.3}", start.max(0.0)), format!("{:.3}", (end - start).max(0.1)));
    run(&["-ss", &ss, "-i", p(input), "-t", &t, "-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero", "-movflags", "+faststart", p(output)])
}

/// The frame at `at` seconds as a 480px JPEG, and its mean brightness (video range: 16 is black).
fn still(input: &Path, at: f64, output: &Path) -> Result<f64> {
    let ss = format!("{:.2}", at.max(0.0));
    let vf = r"format=yuv420p,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file='pipe\:1',scale=480:-2";
    let out = run_out(&["-ss", &ss, "-i", p(input), "-frames:v", "1", "-vf", vf, "-q:v", "5", p(output)])?;
    Ok(out.lines().find_map(|l| l.split("YAVG=").nth(1)).and_then(|v| v.trim().parse().ok()).unwrap_or(255.0))
}

/// A thumbnail from near `at` seconds. A black frame (the game minimized, a loading screen) makes
/// a useless thumbnail, so points before and after it are tried until one shows something; when
/// all of them are dark, the frame at `at` is kept.
pub fn thumbnail(input: &Path, at: f64, duration: f64, output: &Path) -> Result<()> {
    const DARK: f64 = 20.0;
    if still(input, at, output)? > DARK || duration <= 0.0 {
        return Ok(());
    }
    let step = duration * 0.07;
    for t in [1.0, -1.0, 2.0, -2.0, 3.0, -3.0].map(|k| at + k * step) {
        if (0.0..duration).contains(&t) && still(input, t, output)? > DARK {
            return Ok(());
        }
    }
    still(input, at, output).map(drop)
}

/// A horizontal strip of `count` keyframe thumbnails, 160px wide each.
pub fn sprite(input: &Path, duration: f64, count: u32, output: &Path) -> Result<()> {
    let count = count.clamp(2, 200);
    let vf = format!("fps={count}/{:.3},scale=160:-2,tile={count}x1", duration.max(1.0));
    run(&["-skip_frame", "nokey", "-i", p(input), "-vf", &vf, "-frames:v", "1", "-fps_mode", "vfr", "-q:v", "6", p(output)])
}

// ---------- encoder speed on this PC ----------
// Pixels per second, so a number measured on one video carries over to other resolutions.
// Measured once with `benchmark`, then kept current by every real conversion.

static SPEEDS: parking_lot::Mutex<Option<(HashMap<String, f64>, Instant)>> = parking_lot::Mutex::new(None);

fn speeds_path() -> PathBuf {
    paths::config_dir().join("encoder-speed.json")
}

fn with_speeds<T>(f: impl FnOnce(&mut HashMap<String, f64>, &mut Instant) -> T) -> T {
    let mut g = SPEEDS.lock();
    let (map, saved) = g.get_or_insert_with(|| {
        let map = std::fs::read(speeds_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        (map, Instant::now())
    });
    f(map, saved)
}

/// Measured speed of `encoder` on this PC in pixels per second, if known.
pub fn speed(encoder: &str) -> Option<f64> {
    with_speeds(|m, _| m.get(encoder).copied())
}

fn note_speed(encoder: &str, pixels_per_sec: f64, save_now: bool) {
    with_speeds(|m, saved| {
        m.entry(encoder.to_string()).and_modify(|v| *v = *v * 0.8 + pixels_per_sec * 0.2).or_insert(pixels_per_sec);
        if save_now || saved.elapsed().as_secs() >= 30 {
            *saved = Instant::now();
            if let Ok(json) = serde_json::to_vec_pretty(m) {
                let _ = std::fs::write(speeds_path(), json);
            }
        }
    });
}

/// Turns ffmpeg's `frame=` progress lines into a speed, skipping gaps (a suspended process).
struct RateMeter {
    pixels: f64,
    last: Option<(u64, Instant)>,
}

impl RateMeter {
    fn frame(&mut self, frame: u64) -> Option<f64> {
        let now = Instant::now();
        let Some((f0, t0)) = self.last else {
            self.last = Some((frame, now));
            return None;
        };
        let dt = now.duration_since(t0).as_secs_f64();
        if dt < 2.0 {
            return None;
        }
        self.last = Some((frame, now));
        (dt < 6.0 && frame > f0).then(|| (frame - f0) as f64 * self.pixels / dt)
    }
}

/// Encode a few seconds of `input` to nowhere and record how fast `encoder` runs here.
pub fn benchmark(encoder: &str, input: &Path, bitrate_kbps: u32) -> Result<f64> {
    let src = probe(input).context(tr!("cannot read the video",
        zh: "无法读取视频", tw: "無法讀取影片",
        es: "no se puede leer el vídeo", fr: "impossible de lire la vidéo",
        ja: "ビデオを読み込めません", ko: "동영상을 읽을 수 없습니다", ru: "не удаётся прочитать видео"))?;
    let fps = (src.fps.round() as u32).clamp(10, 240);
    let mut cmd = crate::win::hidden_command_low(paths::ffmpeg());
    cmd.env("SVT_LOG", "1");
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostats", "-progress", "pipe:1", "-stats_period", "0.25"]);
    if family(encoder) == "nvenc" {
        cmd.args(["-hwaccel", "cuda", "-hwaccel_output_format", "cuda"]);
    }
    // Long enough for any machine; the measurement below stops after a fixed time instead.
    let start = (src.duration / 4.0).min(120.0);
    cmd.args(["-ss", &format!("{start:.1}"), "-t", "120"]).arg("-i").arg(input);
    cmd.args(["-map", "0:v:0"]).args(video_encoder_args(encoder, "performance", bitrate_kbps, fps)).args(["-f", "null", "-"]);
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().context(crate::i18n::ffmpeg_start_failed())?;
    // Skip the warm-up (encoder setup, first frames), then time the steady part.
    let mut first: Option<(u64, Instant)> = None;
    let mut last: Option<(u64, Instant)> = None;
    let begun = Instant::now();
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(|l| l.ok()) {
        let Some(f) = line.strip_prefix("frame=").and_then(|f| f.trim().parse::<u64>().ok()) else { continue };
        if f == 0 || begun.elapsed().as_secs_f64() < 1.5 {
            continue;
        }
        let now = Instant::now();
        let (_, t0) = *first.get_or_insert((f, now));
        last = Some((f, now));
        // Five seconds of steady encoding is plenty, on a fast or a slow machine.
        if now.duration_since(t0).as_secs_f64() >= 5.0 {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let (Some((f0, t0)), Some((f1, t1))) = (first, last) else { bail!("the test encode produced no frames") };
    let dt = t1.duration_since(t0).as_secs_f64();
    if f1 <= f0 || dt < 0.5 {
        bail!("the test encode was too short to measure");
    }
    let pps = (f1 - f0) as f64 * src.width as f64 * src.height as f64 / dt;
    note_speed(encoder, pps, true);
    log::info!("{encoder}: {:.0} fps on {}x{} in a test encode", pps / (src.width as f64 * src.height as f64), src.width, src.height);
    Ok(pps)
}

#[derive(Clone, Debug, Default, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec: String,
}

pub fn probe(input: &Path) -> Result<Probe> {
    let out = hidden_command(paths::ffprobe())
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=codec_name,width,height,avg_frame_rate:format=duration", "-of", "json", p(input)])
        .stdin(Stdio::null())
        .output()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let st = &v["streams"][0];
    let fps = st["avg_frame_rate"].as_str().and_then(|r| {
        let (n, d) = r.split_once('/')?;
        let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
        (d > 0.0).then(|| n / d)
    });
    Ok(Probe {
        duration: v["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0),
        width: st["width"].as_u64().unwrap_or(0) as u32,
        height: st["height"].as_u64().unwrap_or(0) as u32,
        fps: fps.unwrap_or(0.0),
        codec: st["codec_name"].as_str().unwrap_or("").to_string(),
    })
}

/// One piece of a render: a time range of a source file.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub file: PathBuf,
    pub start: f64,
    pub end: f64,
    /// 0..=200 (percent)
    pub volume: f64,
    /// fit | fill | stretch
    pub crop: String,
}

pub struct RenderSpec {
    pub segments: Vec<Segment>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub encoder: String,
    pub bitrate_kbps: u32,
    pub master_volume: f64,
    pub watermark: bool,
    pub output: PathBuf,
}

/// How a `-progress` run of ffmpeg ended.
enum Ended {
    Done,
    /// Cancelled, or `on_line` asked to stop: ffmpeg was killed.
    Stopped,
    /// ffmpeg failed; its error output.
    Failed(String),
}

/// Run an ffmpeg command that reports with `-progress pipe:1`, handing each line to `on_line`
/// (false stops it). While `paused` is set the process is suspended in place, losing no work.
fn run_progress(cmd: &mut std::process::Command, cancel: &AtomicBool, paused: Option<&AtomicBool>, mut on_line: impl FnMut(&str) -> bool) -> Result<Ended> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().context(crate::i18n::ffmpeg_start_failed())?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut s);
        s
    });
    let pid = child.id();
    let finished = AtomicBool::new(false);
    let stopped = std::thread::scope(|scope| {
        if let Some(paused) = paused {
            scope.spawn(|| {
                let mut suspended = false;
                while !finished.load(Ordering::Relaxed) {
                    // A cancel resumes it, so the read loop below sees the cancel.
                    let want = paused.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed);
                    if want != suspended && crate::win::suspend_process(pid, want) {
                        suspended = want;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                if suspended {
                    crate::win::suspend_process(pid, false);
                }
            });
        }
        let stopped = BufReader::new(stdout).lines().map_while(|l| l.ok()).any(|line| cancel.load(Ordering::Relaxed) || !on_line(&line));
        finished.store(true, Ordering::Relaxed);
        stopped
    });
    if stopped {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(Ended::Stopped);
    }
    let status = child.wait()?;
    let err = err_thread.join().unwrap_or_default();
    Ok(if status.success() { Ended::Done } else { Ended::Failed(err.trim().to_string()) })
}

/// Seconds written so far, from a `-progress` line.
fn out_time(line: &str) -> Option<f64> {
    line.strip_prefix("out_time_us=")?.trim().parse::<f64>().ok().map(|us| us / 1e6)
}

const PROGRESS_ARGS: [&str; 7] = ["-hide_banner", "-loglevel", "error", "-y", "-nostats", "-progress", "pipe:1"];

impl RenderSpec {
    /// The video filters every segment gets: fit into the (even) frame, frame rate, watermark.
    fn video_filter(&self, seg: &Segment) -> String {
        let (w, h) = (self.width / 2 * 2, self.height / 2 * 2);
        let fit = match seg.crop.as_str() {
            "fill" => format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}"),
            "stretch" => format!("scale={w}:{h}"),
            _ => format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2"),
        };
        let mut vf = format!("{fit},setsar=1,fps={}", self.fps);
        if self.watermark {
            let font = r"C\:/Windows/Fonts/segoeuib.ttf";
            vf.push_str(&format!(
                ",drawtext=fontfile='{font}':text='Matt Game Recorder':fontcolor=white@0.55:fontsize={fs}:x=w-tw-{m}:y=h-th-{m}",
                fs = (h / 36).max(12),
                m = (h / 40).max(8)
            ));
        }
        vf
    }

    fn volume(&self, seg: &Segment) -> f64 {
        seg.volume / 100.0 * self.master_volume / 100.0
    }

    fn encoder_args(&self) -> Vec<String> {
        let mut a = video_encoder_args(&self.encoder, "quality", self.bitrate_kbps, self.fps);
        if family(&self.encoder) != "cpu" {
            a.extend(["-pix_fmt".into(), "yuv420p".into()]);
        }
        a.extend(audio_encoder_args());
        a
    }
}

fn export_failed(err: &str) -> anyhow::Error {
    anyhow::anyhow!(tr!("export failed: {err}",
        zh: "导出失败：{err}", tw: "匯出失敗：{err}",
        es: "error al exportar: {err}", fr: "échec de l'export : {err}",
        ja: "エクスポートに失敗しました: {err}", ko: "내보내기 실패: {err}", ru: "ошибка экспорта: {err}"))
}

/// Re-encode segments into one file in a single ffmpeg run (trim, fit to the frame, volume,
/// concat). `progress` receives 0..1.
pub fn render(spec: &RenderSpec, cancel: &AtomicBool, mut progress: impl FnMut(f64)) -> Result<()> {
    if spec.segments.is_empty() {
        bail!(tr!("nothing to render",
            zh: "没有可渲染的内容", tw: "沒有可算繪的內容",
            es: "no hay nada que renderizar", fr: "rien à générer",
            ja: "書き出す内容がありません", ko: "렌더링할 내용이 없습니다", ru: "нечего обрабатывать"));
    }
    let mut cmd = hidden_command(paths::ffmpeg());
    cmd.args(PROGRESS_ARGS);
    let mut inputs: Vec<&Path> = Vec::new();
    for seg in &spec.segments {
        if !inputs.contains(&seg.file.as_path()) {
            inputs.push(&seg.file);
            cmd.arg("-i").arg(&seg.file);
        }
    }
    let mut graph = String::new();
    for (k, seg) in spec.segments.iter().enumerate() {
        let idx = inputs.iter().position(|i| *i == seg.file.as_path()).unwrap();
        graph.push_str(&format!(
            "[{idx}:v]trim=start={s:.3}:end={e:.3},setpts=PTS-STARTPTS,{vf}[v{k}];\
             [{idx}:a]atrim=start={s:.3}:end={e:.3},asetpts=PTS-STARTPTS,volume={vol:.3}[a{k}];",
            s = seg.start,
            e = seg.end,
            vf = spec.video_filter(seg),
            vol = spec.volume(seg),
        ));
    }
    for k in 0..spec.segments.len() {
        graph.push_str(&format!("[v{k}][a{k}]"));
    }
    graph.push_str(&format!("concat=n={}:v=1:a=1[v][a]", spec.segments.len()));
    cmd.arg("-filter_complex").arg(&graph).args(["-map", "[v]", "-map", "[a]"]);
    cmd.args(spec.encoder_args()).args(["-movflags", "+faststart"]).arg(&spec.output);
    let total: f64 = spec.segments.iter().map(|s| (s.end - s.start).max(0.0)).sum();
    let ended = run_progress(&mut cmd, cancel, None, |line| {
        if let Some(t) = out_time(line) {
            progress((t / total.max(0.001)).clamp(0.0, 1.0));
        }
        true
    })?;
    match ended {
        Ended::Done => {
            progress(1.0);
            Ok(())
        }
        Ended::Stopped => {
            let _ = std::fs::remove_file(&spec.output);
            bail!("cancelled")
        }
        Ended::Failed(err) => {
            let _ = std::fs::remove_file(&spec.output);
            Err(export_failed(&err))
        }
    }
}

pub fn has_audio(input: &Path) -> bool {
    hidden_command(paths::ffprobe())
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=index", "-of", "csv=p=0", p(input)])
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
}

/// Like `render`, for moments picked out of long recordings (supercuts): every segment is cut on
/// its own, seeking straight to it, then the parts are joined without re-encoding. `render`
/// decodes each source from its start up to the cut, which is slow for late moments and many
/// files. A source without sound gets silence.
pub fn render_parts(spec: &RenderSpec, cancel: &AtomicBool, mut progress: impl FnMut(f64)) -> Result<()> {
    if spec.segments.is_empty() {
        bail!(tr!("nothing to render",
            zh: "没有可渲染的内容", tw: "沒有可算繪的內容",
            es: "no hay nada que renderizar", fr: "rien à générer",
            ja: "書き出す内容がありません", ko: "렌더링할 내용이 없습니다", ru: "нечего обрабатывать"));
    }
    let parts_dir = spec.output.with_extension("parts");
    std::fs::create_dir_all(&parts_dir)?;
    let result = render_parts_in(spec, &parts_dir, cancel, &mut progress);
    let _ = std::fs::remove_dir_all(&parts_dir);
    if result.is_err() {
        let _ = std::fs::remove_file(&spec.output);
    }
    result
}

fn render_parts_in(spec: &RenderSpec, parts_dir: &Path, cancel: &AtomicBool, progress: &mut impl FnMut(f64)) -> Result<()> {
    let total: f64 = spec.segments.iter().map(|s| (s.end - s.start).max(0.1)).sum();
    let mut done = 0.0;
    let mut list = String::new();
    let mut sound: HashMap<&Path, bool> = HashMap::new();
    for (k, seg) in spec.segments.iter().enumerate() {
        let dur = (seg.end - seg.start).max(0.1);
        let vf = spec.video_filter(seg);
        let part = parts_dir.join(format!("{k:04}.mp4"));
        let mut cmd = hidden_command(paths::ffmpeg());
        cmd.args(PROGRESS_ARGS);
        cmd.args(["-ss", &format!("{:.3}", seg.start.max(0.0)), "-t", &format!("{dur:.3}")]).arg("-i").arg(&seg.file);
        if *sound.entry(&seg.file).or_insert_with(|| has_audio(&seg.file)) {
            cmd.args(["-filter_complex", &format!("[0:v]{vf}[v];[0:a]volume={:.3},aresample=48000[a]", spec.volume(seg))]);
        } else {
            cmd.args(["-f", "lavfi", "-t", &format!("{dur:.3}"), "-i", "anullsrc=r=48000:cl=stereo"]);
            cmd.args(["-filter_complex", &format!("[0:v]{vf}[v];[1:a]anull[a]")]);
        }
        cmd.args(["-map", "[v]", "-map", "[a]"]).args(spec.encoder_args()).arg(&part);
        let ended = run_progress(&mut cmd, cancel, None, |line| {
            if let Some(t) = out_time(line) {
                // The join at the end takes a moment too: keep the last 2% for it.
                progress(((done + t.min(dur)) / total * 0.98).clamp(0.0, 0.98));
            }
            true
        })?;
        match ended {
            Ended::Done => {}
            Ended::Stopped => bail!("cancelled"),
            Ended::Failed(err) => return Err(export_failed(&err)),
        }
        done += dur;
        list.push_str(&format!("file '{}'\n", part.to_string_lossy().replace('\\', "/").replace('\'', "'\\''")));
    }
    let list_path = parts_dir.join("parts.txt");
    std::fs::write(&list_path, list)?;
    run(&["-f", "concat", "-safe", "0", "-i", p(&list_path), "-c", "copy", "-movflags", "+faststart", p(&spec.output)])?;
    progress(1.0);
    Ok(())
}

/// Re-encode a whole video into this app's format (AV1/HEVC/H.264 + Opus), keeping its size and
/// frame rate. NVIDIA: decode and encode both stay on the GPU. Runs at below-normal priority and
/// can be paused. Returns Ok(false) when `abort` turned true (e.g. a match started) and the file
/// was dropped.
#[allow(clippy::too_many_arguments)] // job plumbing: cancel, pause and abort are separate signals
pub fn convert(
    input: &Path,
    output: &Path,
    encoder: &str,
    bitrate_kbps: u32,
    cancel: &AtomicBool,
    paused: &AtomicBool,
    abort: impl Fn() -> bool,
    mut progress: impl FnMut(f64),
) -> Result<bool> {
    let src = probe(input).context(tr!("cannot read the video",
        zh: "无法读取视频", tw: "無法讀取影片",
        es: "no se puede leer el vídeo", fr: "impossible de lire la vidéo",
        ja: "ビデオを読み込めません", ko: "동영상을 읽을 수 없습니다", ru: "не удаётся прочитать видео"))?;
    if src.duration <= 0.0 {
        bail!(tr!("video has no duration",
            zh: "视频没有时长", tw: "影片沒有時長",
            es: "el vídeo no tiene duración", fr: "la vidéo n'a pas de durée",
            ja: "ビデオに長さがありません", ko: "동영상에 길이 정보가 없습니다", ru: "у видео нет длительности"));
    }
    let fps = (src.fps.round() as u32).clamp(10, 240);
    let gpu = family(encoder) == "nvenc";
    let mut run_once = |hw: bool| -> Result<bool> {
        let mut cmd = crate::win::hidden_command_low(paths::ffmpeg());
        // SVT-AV1 logs on its own; errors only.
        cmd.env("SVT_LOG", "1");
        cmd.args(PROGRESS_ARGS);
        if hw {
            cmd.args(["-hwaccel", "cuda", "-hwaccel_output_format", "cuda"]);
        }
        cmd.arg("-i").arg(input);
        // First video and first audio track (Outplayed's first audio track is the full mix).
        cmd.args(["-map", "0:v:0", "-map", "0:a:0?", "-map_metadata", "-1"]);
        cmd.args(video_encoder_args(encoder, "performance", bitrate_kbps, fps));
        cmd.args(audio_encoder_args());
        cmd.args(["-movflags", "+faststart"]).arg(output);
        let mut meter = RateMeter { pixels: src.width as f64 * src.height as f64, last: None };
        let ended = run_progress(&mut cmd, cancel, Some(paused), |line| {
            if let Some(pps) = line.strip_prefix("frame=").and_then(|f| f.trim().parse::<u64>().ok()).and_then(|f| meter.frame(f)) {
                note_speed(encoder, pps, false);
            }
            if let Some(t) = out_time(line) {
                progress((t / src.duration).clamp(0.0, 1.0));
            }
            !abort()
        })?;
        match ended {
            Ended::Done => Ok(true),
            Ended::Stopped => {
                let _ = std::fs::remove_file(output);
                if cancel.load(Ordering::Relaxed) {
                    bail!("cancelled");
                }
                Ok(false)
            }
            Ended::Failed(err) => {
                let _ = std::fs::remove_file(output);
                bail!(tr!("conversion failed: {err}",
                    zh: "转换失败：{err}", tw: "轉換失敗：{err}",
                    es: "error en la conversión: {err}", fr: "échec de la conversion : {err}",
                    ja: "変換に失敗しました: {err}", ko: "변환 실패: {err}", ru: "ошибка конвертации: {err}"))
            }
        }
    };
    match run_once(gpu) {
        // Some sources cannot be GPU-decoded; retry with CPU decoding.
        Err(e) if gpu && !cancel.load(Ordering::Relaxed) => {
            log::warn!("GPU decode failed for {}, retrying on CPU: {e:#}", input.display());
            run_once(false)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mgr-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A test video of `secs` seconds, with a tone or without sound.
    fn source(dir: &Path, name: &str, secs: u32, sound: bool) -> PathBuf {
        let out = dir.join(name);
        let d = secs.to_string();
        let mut args = vec!["-f", "lavfi", "-i", "testsrc2=s=320x180:r=30", "-t", &d];
        if sound {
            args.extend(["-f", "lavfi", "-i", "sine=f=440:r=48000", "-t", &d]);
        }
        args.extend(["-c:v", "libx264", "-g", "30", "-pix_fmt", "yuv420p", p(&out)]);
        run(&args).unwrap();
        out
    }

    fn spec(dir: &Path, segments: Vec<Segment>) -> RenderSpec {
        RenderSpec { segments, width: 320, height: 180, fps: 30, encoder: "libx264".into(), bitrate_kbps: 2000, master_volume: 100.0, watermark: true, output: dir.join("out.mp4") }
    }

    fn seg(file: &Path, start: f64, end: f64) -> Segment {
        Segment { file: file.to_path_buf(), start, end, volume: 100.0, crop: "fit".into() }
    }

    #[test]
    fn render_joins_segments() {
        let dir = scratch("render");
        let (a, b) = (source(&dir, "a.mp4", 6, true), source(&dir, "b.mp4", 6, true));
        let spec = spec(&dir, vec![seg(&a, 1.0, 3.0), seg(&b, 2.0, 5.0)]);
        let mut last = 0.0;
        render(&spec, &AtomicBool::new(false), |p| last = p).unwrap();
        let out = probe(&spec.output).unwrap();
        let sound = has_audio(&spec.output);
        let _ = std::fs::remove_dir_all(&dir);
        assert!((out.duration - 5.0).abs() < 0.3, "duration {}", out.duration);
        assert!(sound && last == 1.0);
    }

    #[test]
    fn render_parts_gives_silent_sources_sound() {
        let dir = scratch("parts");
        let (a, b) = (source(&dir, "a.mp4", 6, true), source(&dir, "b.mp4", 6, false));
        let spec = spec(&dir, vec![seg(&a, 1.0, 3.0), seg(&b, 2.0, 5.0)]);
        let mut last = 0.0;
        render_parts(&spec, &AtomicBool::new(false), |p| last = p).unwrap();
        let out = probe(&spec.output).unwrap();
        let sound = has_audio(&spec.output);
        let parts_left = spec.output.with_extension("parts").exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!((out.duration - 5.0).abs() < 0.3, "duration {}", out.duration);
        assert!(sound && last == 1.0 && !parts_left);
    }

    #[test]
    fn cancelled_render_leaves_nothing() {
        let dir = scratch("cancel");
        let a = source(&dir, "a.mp4", 4, true);
        let spec = spec(&dir, vec![seg(&a, 0.0, 4.0)]);
        let r = render(&spec, &AtomicBool::new(true), |_| {});
        let r2 = render_parts(&spec, &AtomicBool::new(true), |_| {});
        let left = spec.output.exists() || spec.output.with_extension("parts").exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(r.is_err() && r2.is_err() && !left);
    }

    #[test]
    fn convert_reencodes_or_drops_on_abort() {
        let dir = scratch("convert");
        let a = source(&dir, "a.mp4", 4, true);
        let (out, never) = (dir.join("c.mp4"), AtomicBool::new(false));
        let done = convert(&a, &out, "libx264", 2000, &never, &never, || false, |_| {}).unwrap();
        let dur = probe(&out).unwrap().duration;
        let dropped = dir.join("d.mp4");
        let kept = convert(&a, &dropped, "libx264", 2000, &never, &never, || true, |_| {}).unwrap();
        let cancelled = convert(&a, &dropped, "libx264", 2000, &AtomicBool::new(true), &never, || false, |_| {});
        let left = dropped.exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(done && (dur - 4.0).abs() < 0.3, "duration {dur}");
        assert!(!kept && cancelled.is_err() && !left);
    }

    /// A 10 s video, black for its first 4 s: the thumbnail asked for at 3 s comes from the lit part.
    #[test]
    fn thumbnail_skips_black_frames() {
        let dir = scratch("thumb");
        let video = dir.join("v.mp4");
        run(&["-f", "lavfi", "-i", "color=c=black:s=320x180:r=10:d=4", "-f", "lavfi", "-i", "color=c=gray:s=320x180:r=10:d=6", "-filter_complex", "[0:v][1:v]concat=n=2:v=1", "-g", "10", p(&video)]).unwrap();
        let thumb = dir.join("t.jpg");
        thumbnail(&video, 3.0, 10.0, &thumb).unwrap();
        let brightness = still(&thumb, 0.0, &dir.join("check.jpg")).unwrap();
        let dark_only = still(&video, 3.0, &dir.join("dark.jpg")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(dark_only < 20.0, "the frame at 3 s is black: {dark_only}");
        assert!(brightness > 20.0, "picked a dark frame: {brightness}");
    }
}
