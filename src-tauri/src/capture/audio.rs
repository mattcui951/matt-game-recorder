//! Audio capture: WASAPI loopback (output device or a single process) plus microphone,
//! mixed into one 48 kHz stereo f32 stream that is fed to ffmpeg through a named pipe.
//!
//! The mixer is clocked by wall time, so the audio track always has exactly as many samples
//! as seconds elapsed. Silence (WASAPI delivers no packets when nothing plays) becomes zeros,
//! which keeps audio and video aligned for the whole match.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{WriteFile, PIPE_ACCESS_OUTBOUND};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT};

pub const RATE: usize = 48_000;
const CHANNELS: usize = 2;
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

pub fn list_devices() -> AudioDevices {
    let _ = wasapi::initialize_mta();
    let list = |dir: Direction| -> Vec<AudioDevice> {
        let Ok(en) = DeviceEnumerator::new() else { return vec![] };
        let Ok(coll) = en.get_device_collection(&dir) else { return vec![] };
        let n = coll.get_nbr_devices().unwrap_or(0);
        (0..n)
            .filter_map(|i| coll.get_device_at_index(i).ok())
            .filter_map(|d| Some(AudioDevice { id: d.get_id().ok()?, name: d.get_friendlyname().ok()? }))
            .collect()
    };
    AudioDevices { outputs: list(Direction::Render), inputs: list(Direction::Capture) }
}

type Ring = Arc<Mutex<VecDeque<f32>>>;

pub struct AudioPipeline {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    pipe_name: String,
}

impl AudioPipeline {
    /// Create the pipe and start capture. ffmpeg must open `pipe_name()` as `-f f32le -ar 48000 -ac 2`.
    pub fn start(cfg: AudioConfig, id: &str) -> anyhow::Result<AudioPipeline> {
        let pipe_name = format!(r"\\.\pipe\mgr-audio-{id}");
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        let mut sources: Vec<(Ring, f32, bool)> = Vec::new();

        if let Some(sys) = cfg.system.clone() {
            let ring: Ring = Arc::default();
            sources.push((ring.clone(), cfg.system_gain, false));
            let stop = stop.clone();
            threads.push(std::thread::Builder::new().name("audio-system".into()).spawn(move || {
                capture_forever(ring, stop, move || open_system(&sys))
            })?);
        }
        if let Some(mic) = cfg.mic.clone() {
            let ring: Ring = Arc::default();
            sources.push((ring.clone(), cfg.mic_gain, cfg.mic_mono));
            let stop = stop.clone();
            threads.push(std::thread::Builder::new().name("audio-mic".into()).spawn(move || {
                capture_forever(ring, stop, move || open_device(&mic, Direction::Capture))
            })?);
        }

        let handle = create_pipe(&pipe_name)?;
        let stop_m = stop.clone();
        threads.push(std::thread::Builder::new().name("audio-mixer".into()).spawn(move || {
            mixer(handle, sources, stop_m);
        })?);
        Ok(AudioPipeline { stop, threads, pipe_name })
    }

    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Unblock a mixer still waiting in ConnectNamedPipe (ffmpeg never opened the pipe).
        let _ = std::fs::OpenOptions::new().read(true).open(&self.pipe_name);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        if !self.threads.is_empty() {
            self.shutdown();
        }
    }
}

fn format() -> WaveFormat {
    WaveFormat::new(32, 32, &SampleType::Float, RATE, CHANNELS, None)
}

fn open_device(id: &str, dir: Direction) -> anyhow::Result<AudioClient> {
    let en = DeviceEnumerator::new()?;
    let device = if id.is_empty() || id == "default" {
        en.get_default_device(&dir)?
    } else {
        en.get_device(id).or_else(|_| en.get_default_device(&dir))?
    };
    let mut client = device.get_iaudioclient()?;
    let (default_period, _) = client.get_device_period()?;
    client.initialize_client(
        &format(),
        &Direction::Capture,
        &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: default_period * 4 },
    )?;
    Ok(client)
}

fn open_system(src: &SystemSource) -> anyhow::Result<AudioClient> {
    match src {
        SystemSource::Device(id) => open_device(id, Direction::Render),
        SystemSource::Process(pid) => {
            let mut client = AudioClient::new_application_loopback_client(*pid, true)?;
            client.initialize_client(
                &format(),
                &Direction::Capture,
                &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 },
            )?;
            Ok(client)
        }
    }
}

/// Capture from a source until stopped, reopening it if the device disappears.
fn capture_forever(ring: Ring, stop: Arc<AtomicBool>, open: impl Fn() -> anyhow::Result<AudioClient>) {
    let _ = wasapi::initialize_mta();
    while !stop.load(Ordering::Relaxed) {
        match open().and_then(|client| run_capture(&client, &ring, &stop)) {
            Ok(()) => return,
            Err(e) => {
                log::warn!("audio source error: {e:#}; retrying");
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn run_capture(client: &AudioClient, ring: &Ring, stop: &AtomicBool) -> anyhow::Result<()> {
    let event = client.set_get_eventhandle()?;
    let capture = client.get_audiocaptureclient()?;
    let mut bytes: VecDeque<u8> = VecDeque::with_capacity(RATE * 8);
    client.start_stream()?;
    while !stop.load(Ordering::Relaxed) {
        // Loopback signals nothing while the device is silent, so a timeout is normal.
        let _ = event.wait_for_event(50);
        loop {
            match capture.get_next_packet_size()? {
                Some(0) => break,
                _ => {
                    capture.read_from_device_to_deque(&mut bytes)?;
                    if capture.get_next_packet_size()?.is_none() {
                        break;
                    }
                }
            }
        }
        if bytes.len() >= 4 {
            let usable = bytes.len() / 4 * 4;
            let (a, b) = bytes.as_slices();
            let mut chunk = Vec::with_capacity(usable);
            chunk.extend_from_slice(a);
            chunk.extend_from_slice(b);
            chunk.truncate(usable);
            bytes.drain(..usable);
            let mut r = ring.lock();
            r.extend(chunk.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)));
            let excess = r.len().saturating_sub(MAX_BACKLOG);
            if excess > 0 {
                r.drain(..excess - excess % CHANNELS);
            }
        }
    }
    let _ = client.stop_stream();
    Ok(())
}

fn create_pipe(name: &str) -> anyhow::Result<isize> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let h = unsafe {
        CreateNamedPipeW(
            PCWSTR(wide.as_ptr()),
            PIPE_ACCESS_OUTBOUND,
            PIPE_TYPE_BYTE | PIPE_WAIT,
            1,
            1 << 20,
            0,
            0,
            None,
        )
    };
    if h == INVALID_HANDLE_VALUE || h.is_invalid() {
        anyhow::bail!("could not create audio pipe {name}");
    }
    Ok(h.0 as isize)
}

fn mixer(handle: isize, sources: Vec<(Ring, f32, bool)>, stop: Arc<AtomicBool>) {
    let h = HANDLE(handle as *mut _);
    // Blocks until ffmpeg opens the pipe (or `shutdown` opens it to wake us).
    let _ = unsafe { ConnectNamedPipe(h, None) };
    if stop.load(Ordering::Relaxed) {
        unsafe {
            let _ = CloseHandle(h);
        }
        return;
    }
    // Drop whatever was captured before ffmpeg was listening.
    for (ring, _, _) in &sources {
        ring.lock().clear();
    }
    let start = Instant::now();
    let mut produced: usize = 0;
    let mut mix: Vec<f32> = Vec::new();
    let mut out: Vec<u8> = Vec::new();
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
        for (ring, gain, mono) in &sources {
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
        out.clear();
        out.reserve(samples * 4);
        for s in &mix {
            out.extend_from_slice(&s.clamp(-1.0, 1.0).to_le_bytes());
        }
        let mut written = 0u32;
        if unsafe { WriteFile(h, Some(&out), Some(&mut written), None) }.is_err() {
            break; // ffmpeg exited
        }
        produced += frames;
    }
    unsafe {
        let _ = CloseHandle(h);
    }
}
