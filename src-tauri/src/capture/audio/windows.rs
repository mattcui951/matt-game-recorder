//! WASAPI sources (loopback of an output device or of one process tree, and microphones) and
//! the named pipe to ffmpeg.

use super::{push_samples, AudioDevice, AudioDevices, Ring, SystemSource, CHANNELS, RATE};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{WriteFile, PIPE_ACCESS_OUTBOUND};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT};

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

/// Loopback capture on its own thread.
pub fn capture_system(src: SystemSource, ring: Ring, stop: Arc<AtomicBool>) -> anyhow::Result<Option<JoinHandle<()>>> {
    let t = std::thread::Builder::new().name("audio-system".into()).spawn(move || capture_forever(ring, stop, move || open_system(&src)))?;
    Ok(Some(t))
}

pub fn capture_mic(id: String, ring: Ring, stop: Arc<AtomicBool>) -> anyhow::Result<JoinHandle<()>> {
    Ok(std::thread::Builder::new().name("audio-mic".into()).spawn(move || capture_forever(ring, stop, move || open_device(&id, Direction::Capture)))?)
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
            push_samples(ring, chunk.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)));
        }
    }
    let _ = client.stop_stream();
    Ok(())
}

/// The server end of the audio pipe, until ffmpeg connects.
pub struct PipeEnd(isize);

/// The connected pipe.
pub struct PipeOut(HANDLE);

// SAFETY: a pipe handle may be used from any thread; only the mixer thread uses it.
unsafe impl Send for PipeEnd {}

impl PipeEnd {
    /// Blocks until ffmpeg opens the pipe (or `unblock_pipe` does).
    pub fn connect(self) -> Option<PipeOut> {
        let h = HANDLE(self.0 as *mut _);
        let _ = unsafe { ConnectNamedPipe(h, None) };
        Some(PipeOut(h))
    }
}

impl std::io::Write for PipeOut {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut written = 0u32;
        unsafe { WriteFile(self.0, Some(buf), Some(&mut written), None) }.map_err(std::io::Error::other)?;
        Ok(written as usize)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for PipeOut {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn create_pipe(id: &str) -> anyhow::Result<(String, PipeEnd)> {
    let name = format!(r"\\.\pipe\mgr-audio-{id}");
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let h = unsafe { CreateNamedPipeW(PCWSTR(wide.as_ptr()), PIPE_ACCESS_OUTBOUND, PIPE_TYPE_BYTE | PIPE_WAIT, 1, 1 << 20, 0, 0, None) };
    if h == INVALID_HANDLE_VALUE || h.is_invalid() {
        anyhow::bail!("could not create audio pipe {name}");
    }
    Ok((name, PipeEnd(h.0 as isize)))
}

/// Wake a mixer still waiting in `connect` by connecting to the pipe ourselves.
pub fn unblock_pipe(name: &str) {
    let _ = std::fs::OpenOptions::new().read(true).open(name);
}

/// Named pipes vanish with their last handle.
pub fn remove_pipe(_name: &str) {}
