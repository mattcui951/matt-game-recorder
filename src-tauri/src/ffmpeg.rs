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
    let out = hidden_command(paths::ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context(tr!("failed to start ffmpeg", "无法启动 ffmpeg"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        bail!(tr!("ffmpeg failed: {err}", "ffmpeg 出错：{err}"));
    }
    Ok(())
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

pub fn thumbnail(input: &Path, at: f64, output: &Path) -> Result<()> {
    let ss = format!("{:.2}", at.max(0.0));
    run(&["-ss", &ss, "-i", p(input), "-frames:v", "1", "-vf", "scale=480:-2", "-q:v", "5", p(output)])
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
struct RateMeter<'a> {
    encoder: &'a str,
    pixels: f64,
    last: Option<(u64, Instant)>,
}

impl RateMeter<'_> {
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
    let src = probe(input).context(tr!("cannot read the video", "无法读取视频"))?;
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
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().context(tr!("failed to start ffmpeg", "无法启动 ffmpeg"))?;
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

/// Re-encode segments into one file (trim, scale/crop to the frame, volume, concat).
/// `progress` receives 0..1. Returns early with an error if `cancel` is set.
pub fn render(spec: &RenderSpec, cancel: &AtomicBool, mut progress: impl FnMut(f64)) -> Result<()> {
    if spec.segments.is_empty() {
        bail!(tr!("nothing to render", "没有可渲染的内容"));
    }
    let (w, h) = (spec.width / 2 * 2, spec.height / 2 * 2);
    let mut cmd = hidden_command(paths::ffmpeg());
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-nostats", "-progress", "pipe:1"]);
    let mut inputs: Vec<&Path> = Vec::new();
    for seg in &spec.segments {
        if !inputs.contains(&seg.file.as_path()) {
            inputs.push(&seg.file);
        }
    }
    for i in &inputs {
        cmd.arg("-i").arg(i);
    }
    let mut graph = String::new();
    let mut total = 0.0;
    for (k, seg) in spec.segments.iter().enumerate() {
        let idx = inputs.iter().position(|i| *i == seg.file.as_path()).unwrap();
        let fit = match seg.crop.as_str() {
            "fill" => format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}"),
            "stretch" => format!("scale={w}:{h}"),
            _ => format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2"),
        };
        let vol = seg.volume / 100.0 * spec.master_volume / 100.0;
        graph.push_str(&format!(
            "[{idx}:v]trim=start={s:.3}:end={e:.3},setpts=PTS-STARTPTS,{fit},setsar=1,fps={fps}[v{k}];\
             [{idx}:a]atrim=start={s:.3}:end={e:.3},asetpts=PTS-STARTPTS,volume={vol:.3}[a{k}];",
            s = seg.start,
            e = seg.end,
            fps = spec.fps
        ));
        total += (seg.end - seg.start).max(0.0);
    }
    for k in 0..spec.segments.len() {
        graph.push_str(&format!("[v{k}][a{k}]"));
    }
    graph.push_str(&format!("concat=n={}:v=1:a=1[cv][ca]", spec.segments.len()));
    let vout = if spec.watermark {
        let font = r"C\:/Windows/Fonts/segoeuib.ttf";
        graph.push_str(&format!(
            ";[cv]drawtext=fontfile='{font}':text='Matt Game Recorder':fontcolor=white@0.55:fontsize={fs}:x=w-tw-{m}:y=h-th-{m}[wv]",
            fs = (h / 36).max(12),
            m = (h / 40).max(8)
        ));
        "[wv]"
    } else {
        "[cv]"
    };
    cmd.arg("-filter_complex").arg(&graph);
    cmd.args(["-map", vout, "-map", "[ca]"]);
    let mut venc = video_encoder_args(&spec.encoder, "quality", spec.bitrate_kbps, spec.fps);
    if family(&spec.encoder) != "cpu" {
        venc.extend(["-pix_fmt".into(), "yuv420p".into()]);
    }
    cmd.args(&venc);
    cmd.args(audio_encoder_args());
    cmd.args(["-movflags", "+faststart"]).arg(&spec.output);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().context(tr!("failed to start ffmpeg", "无法启动 ffmpeg"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut s);
        s
    });
    for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&spec.output);
            bail!("cancelled");
        }
        if let Some(us) = line.strip_prefix("out_time_us=") {
            if let Ok(us) = us.trim().parse::<f64>() {
                progress((us / 1e6 / total.max(0.001)).clamp(0.0, 1.0));
            }
        }
    }
    let status = child.wait()?;
    let err = err_thread.join().unwrap_or_default();
    if !status.success() {
        let _ = std::fs::remove_file(&spec.output);
        let err = err.trim();
        bail!(tr!("export failed: {err}", "导出失败：{err}"));
    }
    progress(1.0);
    Ok(())
}

/// Whether a video has an audio stream.
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
/// files. Parts share size, frame rate and codecs, so they join cleanly; a source without sound
/// gets silence.
pub fn render_parts(spec: &RenderSpec, cancel: &AtomicBool, mut progress: impl FnMut(f64)) -> Result<()> {
    if spec.segments.is_empty() {
        bail!(tr!("nothing to render", "没有可渲染的内容"));
    }
    let (w, h) = (spec.width / 2 * 2, spec.height / 2 * 2);
    let parts_dir = spec.output.with_extension("parts");
    std::fs::create_dir_all(&parts_dir)?;
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&parts_dir);
    };
    let total: f64 = spec.segments.iter().map(|s| (s.end - s.start).max(0.1)).sum();
    let mut done = 0.0;
    let mut list = String::new();
    let mut audio: Vec<(PathBuf, bool)> = Vec::new();
    for (k, seg) in spec.segments.iter().enumerate() {
        let dur = (seg.end - seg.start).max(0.1);
        let sound = match audio.iter().find(|(f, _)| *f == seg.file) {
            Some((_, a)) => *a,
            None => {
                let a = has_audio(&seg.file);
                audio.push((seg.file.clone(), a));
                a
            }
        };
        let fit = match seg.crop.as_str() {
            "fill" => format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}"),
            "stretch" => format!("scale={w}:{h}"),
            _ => format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2"),
        };
        let mut vf = format!("{fit},setsar=1,fps={}", spec.fps);
        if spec.watermark {
            let font = r"C\:/Windows/Fonts/segoeuib.ttf";
            vf.push_str(&format!(
                ",drawtext=fontfile='{font}':text='Matt Game Recorder':fontcolor=white@0.55:fontsize={fs}:x=w-tw-{m}:y=h-th-{m}",
                fs = (h / 36).max(12),
                m = (h / 40).max(8)
            ));
        }
        let vol = seg.volume / 100.0 * spec.master_volume / 100.0;
        let part = parts_dir.join(format!("{k:04}.mp4"));
        let mut cmd = hidden_command(paths::ffmpeg());
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-nostats", "-progress", "pipe:1"]);
        cmd.args(["-ss", &format!("{:.3}", seg.start.max(0.0)), "-t", &format!("{dur:.3}")]).arg("-i").arg(&seg.file);
        if sound {
            cmd.args(["-filter_complex", &format!("[0:v]{vf}[v];[0:a]volume={vol:.3},aresample=48000[a]")]);
        } else {
            cmd.args(["-f", "lavfi", "-t", &format!("{dur:.3}"), "-i", "anullsrc=r=48000:cl=stereo"]);
            cmd.args(["-filter_complex", &format!("[0:v]{vf}[v];[1:a]anull[a]")]);
        }
        cmd.args(["-map", "[v]", "-map", "[a]"]);
        let mut venc = video_encoder_args(&spec.encoder, "quality", spec.bitrate_kbps, spec.fps);
        if family(&spec.encoder) != "cpu" {
            venc.extend(["-pix_fmt".into(), "yuv420p".into()]);
        }
        cmd.args(&venc).args(audio_encoder_args()).arg(&part);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().context(tr!("failed to start ffmpeg", "无法启动 ffmpeg"))?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let err_thread = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut s);
            s
        });
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                cleanup();
                bail!("cancelled");
            }
            if let Some(us) = line.strip_prefix("out_time_us=") {
                if let Ok(us) = us.trim().parse::<f64>() {
                    // The join at the end takes a moment too: keep the last 2% for it.
                    progress(((done + (us / 1e6).min(dur)) / total * 0.98).clamp(0.0, 0.98));
                }
            }
        }
        let status = child.wait()?;
        let err = err_thread.join().unwrap_or_default();
        if !status.success() {
            cleanup();
            let err = err.trim();
            bail!(tr!("export failed: {err}", "导出失败：{err}"));
        }
        done += dur;
        list.push_str(&format!("file '{}'\n", part.to_string_lossy().replace('\\', "/").replace('\'', "'\\''")));
    }
    let list_path = parts_dir.join("parts.txt");
    std::fs::write(&list_path, list)?;
    let out = hidden_command(paths::ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(&spec.output)
        .stdin(Stdio::null())
        .output()
        .context(tr!("failed to start ffmpeg", "无法启动 ffmpeg"));
    cleanup();
    let out = out?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&spec.output);
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        bail!(tr!("export failed: {err}", "导出失败：{err}"));
    }
    progress(1.0);
    Ok(())
}

/// Re-encode a whole video into this app's format (AV1/HEVC/H.264 + Opus), keeping its size and
/// frame rate. NVIDIA: decode and encode both stay on the GPU. Runs at below-normal priority.
/// While `paused` is set the ffmpeg process is suspended in place, losing no work.
/// Returns Ok(false) when `abort` turned true (e.g. a match started) and the file was dropped.
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
    let src = probe(input).context(tr!("cannot read the video", "无法读取视频"))?;
    if src.duration <= 0.0 {
        bail!(tr!("video has no duration", "视频没有时长"));
    }
    let fps = (src.fps.round() as u32).clamp(10, 240);
    let gpu = family(encoder) == "nvenc";
    let mut run_once = |hw: bool| -> Result<bool> {
        let mut cmd = crate::win::hidden_command_low(paths::ffmpeg());
        // SVT-AV1 logs on its own; errors only.
        cmd.env("SVT_LOG", "1");
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-nostats", "-progress", "pipe:1"]);
        if hw {
            cmd.args(["-hwaccel", "cuda", "-hwaccel_output_format", "cuda"]);
        }
        cmd.arg("-i").arg(input);
        // First video and first audio track (Outplayed's first audio track is the full mix).
        cmd.args(["-map", "0:v:0", "-map", "0:a:0?", "-map_metadata", "-1"]);
        cmd.args(video_encoder_args(encoder, "performance", bitrate_kbps, fps));
        cmd.args(audio_encoder_args());
        cmd.args(["-movflags", "+faststart"]).arg(output);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().context(tr!("failed to start ffmpeg", "无法启动 ffmpeg"))?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let err_thread = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut s);
            s
        });
        let pid = child.id();
        let finished = AtomicBool::new(false);
        let stopped_early = std::thread::scope(|scope| {
            // Pause = suspend ffmpeg; resume continues exactly where it stopped.
            scope.spawn(|| {
                let mut suspended = false;
                while !finished.load(Ordering::Relaxed) {
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
            let mut early = false;
            let mut meter = RateMeter { encoder, pixels: src.width as f64 * src.height as f64, last: None };
            for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                if cancel.load(Ordering::Relaxed) || abort() {
                    early = true;
                    break;
                }
                if let Some(f) = line.strip_prefix("frame=").and_then(|f| f.trim().parse::<u64>().ok()) {
                    if let Some(pps) = meter.frame(f) {
                        note_speed(meter.encoder, pps, false);
                    }
                }
                if let Some(us) = line.strip_prefix("out_time_us=") {
                    if let Ok(us) = us.trim().parse::<f64>() {
                        progress((us / 1e6 / src.duration).clamp(0.0, 1.0));
                    }
                }
            }
            finished.store(true, Ordering::Relaxed);
            early
        });
        if stopped_early {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(output);
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            return Ok(false);
        }
        let status = child.wait()?;
        let err = err_thread.join().unwrap_or_default();
        if !status.success() {
            let _ = std::fs::remove_file(output);
            let err = err.trim();
            bail!(tr!("conversion failed: {err}", "转换失败：{err}"));
        }
        Ok(true)
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
