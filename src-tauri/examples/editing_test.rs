//! Smoke test for the editing pipeline on a real recording:
//! `cargo run --example editing_test -- <input.mp4> <out_dir>`

use mgr_lib::ffmpeg::{self, RenderSpec, Segment};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let input = PathBuf::from(&args[1]);
    let out = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&out)?;
    let probe = ffmpeg::probe(&input)?;
    println!("source: {:.1}s {}x{} {}", probe.duration, probe.width, probe.height, probe.codec);

    let t = Instant::now();
    ffmpeg::cut_copy(&input, 27.0, 57.0, &out.join("trim.mp4"))?;
    println!("quick trim: {:.2}s -> {:.1}s long", t.elapsed().as_secs_f64(), ffmpeg::probe(&out.join("trim.mp4"))?.duration);

    let cancel = AtomicBool::new(false);
    let seg = |a: f64, b: f64, crop: &str| Segment { file: input.clone(), start: a, end: b, volume: 100.0, crop: crop.into() };
    for (name, w, h, enc, segs) in [
        ("supercut.mp4", probe.width, probe.height, "av1_nvenc", vec![seg(18.0, 27.0, "fit"), seg(35.0, 42.0, "fit")]),
        ("export_9x16.mp4", 1080, 1920, "h264_nvenc", vec![seg(20.0, 26.0, "fill"), seg(60.0, 64.0, "fit")]),
    ] {
        let t = Instant::now();
        let mut last = 0.0;
        ffmpeg::render(
            &RenderSpec { segments: segs, width: w, height: h, fps: 60, encoder: enc.into(), bitrate_kbps: 8000, master_volume: 100.0, watermark: name.contains("export"), output: out.join(name) },
            &cancel,
            |p| last = p,
        )?;
        let p = ffmpeg::probe(&out.join(name))?;
        println!("{name}: {:.2}s, progress reached {:.0}%, result {:.1}s {}x{} {}", t.elapsed().as_secs_f64(), last * 100.0, p.duration, p.width, p.height, p.codec);
    }
    let t = Instant::now();
    ffmpeg::sprite(&input, probe.duration, 40, &out.join("sprite.jpg"))?;
    println!("sprite: {:.2}s", t.elapsed().as_secs_f64());
    Ok(())
}
