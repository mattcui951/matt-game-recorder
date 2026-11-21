//! Audio capture: the system sound (an output device or the game alone) plus microphone, mixed
//! into one 48 kHz stereo f32 stream that is fed to ffmpeg through a pipe (a named pipe on
//! Windows, a FIFO on macOS). The sources are per platform: WASAPI on Windows; on macOS the
//! system sound arrives with the ScreenCaptureKit video stream and the microphone through Core
//! Audio.
//!
//! The mixer is clocked by wall time, so the audio track always has exactly as many samples
//! as seconds elapsed. Silence (sources deliver nothing when nothing plays) becomes zeros,
//! which keeps audio and video aligned for the whole match.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as imp;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use self::macos as imp;

pub use imp::list_devices;

pub const RATE: usize = 48_000;
pub const CHANNELS: usize = 2;
/// A source further ahead than this is trimmed so latency cannot creep up.
const MAX_BACKLOG: usize = RATE * CHANNELS * 3 / 10;

#[derive(Clone, Debug)]
pub enum SystemSource {
    /// Endpoint id, or "default".
    Device(String),
    /// Capture one process tree (the game) only.
    Process(u32),
}

#[derive(Clone, Debug)]
pub struct AudioConfig {
    pub system: Option<SystemSource>,
    pub system_gain: f32,
    /// Endpoint id, or "default".
    pub mic: Option<String>,
    pub mic_gain: f32,
    pub mic_mono: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AudioDevices {
    pub outputs: Vec<AudioDevice>,
    pub inputs: Vec<AudioDevice>,
}

/// Interleaved stereo samples waiting for the mixer.
pub type Ring = Arc<Mutex<VecDeque<f32>>>;

/// Queue interleaved stereo samples from a source, dropping the oldest past the backlog limit.
pub fn push_samples(ring: &Ring, samples: impl IntoIterator<Item = f32>) {
    let mut r = ring.lock();
    r.extend(samples);
    let excess = r.len().saturating_sub(MAX_BACKLOG);
    if excess > 0 {
        r.drain(..excess - excess % CHANNELS);
    }
}

pub struct AudioPipeline {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    pipe_name: String,
    /// Where the video capture delivers the system sound, when it carries it (macOS).
    system_ring: Option<Ring>,
}

impl AudioPipeline {
    /// Create the pipe and start capture. ffmpeg must open `pipe_name()` as `-f f32le -ar 48000 -ac 2`.
    pub fn start(cfg: AudioConfig, id: &str) -> anyhow::Result<AudioPipeline> {
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        let mut sources: Vec<(Ring, f32, bool)> = Vec::new();
        let mut system_ring = None;

        if let Some(sys) = cfg.system.clone() {
            let ring: Ring = Arc::default();
            sources.push((ring.clone(), cfg.system_gain, false));
            match imp::capture_system(sys, ring.clone(), stop.clone())? {
                Some(t) => threads.push(t),
                None => system_ring = Some(ring),
            }
        }
        if let Some(mic) = cfg.mic.clone() {
            let ring: Ring = Arc::default();
            sources.push((ring.clone(), cfg.mic_gain, cfg.mic_mono));
            threads.push(imp::capture_mic(mic, ring, stop.clone())?);
        }

        let (pipe_name, pipe) = imp::create_pipe(id)?;
        let stop_m = stop.clone();
        threads.push(std::thread::Builder::new().name("audio-mixer".into()).spawn(move || {
            // Blocks until ffmpeg opens the pipe (or `shutdown` opens it to wake us).
            let Some(mut out) = pipe.connect() else { return };
            if !stop_m.load(Ordering::Relaxed) {
                mixer(&mut out, &sources, &stop_m);
            }
        })?);
        Ok(AudioPipeline { stop, threads, pipe_name, system_ring })
    }

    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    /// The ring the video capture fills with the system sound, if this platform works that way
    /// and system sound is on.
    pub fn system_ring(&self) -> Option<Ring> {
        self.system_ring.clone()
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Unblock a mixer still waiting for ffmpeg to open the pipe (it never did).
        imp::unblock_pipe(&self.pipe_name);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        imp::remove_pipe(&self.pipe_name);
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        if !self.threads.is_empty() {
            self.shutdown();
        }
    }
}

fn mixer(out: &mut dyn Write, sources: &[(Ring, f32, bool)], stop: &AtomicBool) {
    // Drop whatever was captured before ffmpeg was listening.
    for (ring, _, _) in sources {
        ring.lock().clear();
    }
    let start = Instant::now();
    let mut produced: usize = 0;
    let mut mix: Vec<f32> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
        let target = (start.elapsed().as_secs_f64() * RATE as f64) as usize;
        let frames = target.saturating_sub(produced);
        if frames == 0 {
            continue;
        }
        let samples = frames * CHANNELS;
        mix.clear();
        mix.resize(samples, 0.0);
        for (ring, gain, mono) in sources {
            let mut r = ring.lock();
            let take = samples.min(r.len() - r.len() % CHANNELS);
            if *mono {
                // Average L/R so a one-sided mic lands in the center.
                let mut it = r.drain(..take);
                let mut i = 0;
                while let (Some(l), Some(rr)) = (it.next(), it.next()) {
                    let m = (l + rr) * 0.5 * gain;
                    mix[i] += m;
                    mix[i + 1] += m;
                    i += 2;
                }
            } else {
                for (i, s) in r.drain(..take).enumerate() {
                    mix[i] += s * gain;
                }
            }
        }
        bytes.clear();
        bytes.reserve(samples * 4);
        for s in &mix {
            bytes.extend_from_slice(&s.clamp(-1.0, 1.0).to_le_bytes());
        }
        if out.write_all(&bytes).is_err() {
            break; // ffmpeg exited
        }
        produced += frames;
    }
}

/// Turns a source's frames (any rate, any channel count) into 48 kHz interleaved stereo, by
/// linear interpolation. Used where the platform hands over audio at the device's own rate
/// (macOS; WASAPI converts by itself).
#[cfg_attr(windows, allow(dead_code))]
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: [f32; 2],
}

#[cfg_attr(windows, allow(dead_code))]
impl Resampler {
    pub fn new(source_rate: u32) -> Self {
        Self { step: source_rate.max(1) as f64 / RATE as f64, pos: 0.0, prev: [0.0; 2] }
    }

    /// Feed `frames` (each `channels` samples wide); stereo output is appended to `out`.
    pub fn push(&mut self, input: &[f32], channels: usize, out: &mut Vec<f32>) {
        let channels = channels.max(1);
        let frame = |i: usize| -> [f32; 2] {
            let f = &input[i * channels..];
            if channels == 1 { [f[0], f[0]] } else { [f[0], f[1]] }
        };
        let n = input.len() / channels;
        if n == 0 {
            return;
        }
        if (self.step - 1.0).abs() < 1e-9 {
            for i in 0..n {
                out.extend_from_slice(&frame(i));
            }
            return;
        }
        // `pos` is the next output's place in this chunk's frames; -1 is the previous chunk's
        // last frame, so interpolation runs on across chunk boundaries.
        let prev = self.prev;
        let at = |i: isize| if i < 0 { prev } else { frame(i as usize) };
        while self.pos < (n - 1) as f64 {
            let i = self.pos.floor() as isize;
            let t = (self.pos - i as f64) as f32;
            let (a, b) = (at(i), at(i + 1));
            out.push(a[0] + (b[0] - a[0]) * t);
            out.push(a[1] + (b[1] - a[1]) * t);
            self.pos += self.step;
        }
        self.pos -= n as f64;
        self.prev = frame(n - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_keeps_48k_as_is_and_widens_mono() {
        let mut r = Resampler::new(48_000);
        let mut out = vec![];
        r.push(&[0.1, 0.2, 0.3], 1, &mut out);
        assert_eq!(out, vec![0.1, 0.1, 0.2, 0.2, 0.3, 0.3]);
    }

    #[test]
    fn resampler_converts_rate_without_drift() {
        let mut r = Resampler::new(44_100);
        let mut out = vec![];
        // One second in uneven chunks, as a device would deliver it.
        let input: Vec<f32> = (0..44_100).map(|i| i as f32 / 44_100.0).collect();
        for chunk in input.chunks(441 * 3 + 7) {
            r.push(chunk, 1, &mut out);
        }
        let frames = out.len() / 2;
        assert!((frames as i64 - 48_000).abs() <= 2, "{frames} frames");
        // A ramp stays a ramp.
        assert!(out.chunks(2).zip(out.chunks(2).skip(1)).all(|(a, b)| b[0] >= a[0] - 1e-6));
    }
}
