//! Manual smoke test: records the primary monitor for N seconds to a full file and a RAM ring,
//! then cuts a clip from the ring.  `cargo run --example capture_test -- <out_dir> [secs]`

use mgr_lib::capture::{audio::{AudioConfig, SystemSource}, CaptureOptions, Recorder, RingOptions, Target};
use std::path::PathBuf;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let out = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| ".".into()));
    let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    let mon = mgr_lib::win::monitors().into_iter().find(|m| m.primary).expect("monitor");
    println!("monitor {:?}", mon);
    println!("encoders: {:?}", mgr_lib::ffmpeg::encoders().iter().filter(|e| e.available).map(|e| &e.id).collect::<Vec<_>>());
    let rec = Recorder::start(CaptureOptions {
        target: Target::Monitor(mon.handle),
        fps: 60,
        size: Some((1920 * mon.width / mon.height / 2 * 2 * 1080 / 1080, 1080)),
        stretch: false,
        cursor: true,
        border: false,
        encoder: "av1_nvenc".into(),
        encoder_preset: "default".into(),
        bitrate_kbps: 8000,
        audio: AudioConfig { system: Some(SystemSource::Device("default".into())), system_gain: 1.0, mic: Some("default".into()), mic_gain: 1.0, mic_mono: false },
        webcam: None,
        file: Some(out.join("full.mp4")),
        ring: Some(RingOptions { in_ram: true, temp_dir: out.clone(), keep_secs: 30.0 }),
    })?;
    let t0 = std::time::Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(500));
        print!("now={:.2} ", rec.now());
    }
    println!();
    let (a, b) = rec.extract(2.0, 5.0, &out.join("clip.mp4"))?;
    println!("clip covers {a:.2}..{b:.2}");
    let fin = rec.stop()?;
    println!("finished: {:?} dur={:.2}", fin.file, fin.duration);
    Ok(())
}
