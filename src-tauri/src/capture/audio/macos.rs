//! macOS sources: the system sound comes with the ScreenCaptureKit stream (see `capture::sck`),
//! which also limits it to the game when asked; the microphone comes through Core Audio (cpal).
//! A FIFO carries the mix to ffmpeg.

use super::{push_samples, AudioDevice, AudioDevices, Resampler, Ring, SystemSource, RATE};
use crate::capture::fifo;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub fn list_devices() -> AudioDevices {
    let host = cpal::default_host();
    let inputs = host
        .input_devices()
        .map(|devices| devices.filter_map(|d| Some(AudioDevice { id: d.id().ok()?.to_string(), name: d.description().ok()?.name().to_string() })).collect())
        .unwrap_or_default();
    // ScreenCaptureKit records what the Mac plays, whichever output it plays on.
    AudioDevices { outputs: vec![], inputs }
}

/// The system sound is delivered by the video capture into the returned ring, so no thread.
pub fn capture_system(_src: SystemSource, _ring: Ring, _stop: Arc<AtomicBool>) -> anyhow::Result<Option<JoinHandle<()>>> {
    Ok(None)
}

pub fn capture_mic(id: String, ring: Ring, stop: Arc<AtomicBool>) -> anyhow::Result<JoinHandle<()>> {
    Ok(std::thread::Builder::new().name("audio-mic".into()).spawn(move || {
        // The stream lives on this thread; a device error ends it and the device is opened again.
        while !stop.load(Ordering::Relaxed) {
            let failed = Arc::new(AtomicBool::new(false));
            match open_mic(&id, &ring, failed.clone()) {
                Ok(stream) => {
                    while !stop.load(Ordering::Relaxed) && !failed.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    drop(stream);
                }
                Err(e) => log::warn!("microphone error: {e:#}; retrying"),
            }
            if !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    })?)
}

fn open_mic(id: &str, ring: &Ring, failed: Arc<AtomicBool>) -> anyhow::Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = id
        .parse::<cpal::DeviceId>()
        .ok()
        .filter(|_| id != "default")
        .and_then(|id| host.device_by_id(&id))
        .or_else(|| host.default_input_device())
        .ok_or_else(|| anyhow::anyhow!("no microphone"))?;
    // 48 kHz when the device offers it, so nothing needs resampling.
    let config = device
        .supported_input_configs()?
        .filter(|c| c.sample_format() == cpal::SampleFormat::F32)
        .find_map(|c| c.try_with_sample_rate(RATE as u32))
        .map_or_else(|| device.default_input_config(), Ok)?;
    let channels = config.channels() as usize;
    let mut resampler = Resampler::new(config.sample_rate());
    let ring = ring.clone();
    let mut out = Vec::new();
    let stream = device.build_input_stream::<f32, _, _>(
        config.config(),
        move |data: &[f32], _| {
            out.clear();
            resampler.push(data, channels, &mut out);
            push_samples(&ring, out.iter().copied());
        },
        move |e| {
            log::warn!("microphone stream error: {e}");
            failed.store(true, Ordering::Relaxed);
        },
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

pub struct PipeEnd(std::path::PathBuf);

impl PipeEnd {
    /// Blocks until ffmpeg opens the FIFO (or `unblock_pipe` does).
    pub fn connect(self) -> Option<std::fs::File> {
        fifo::open_writer(&self.0)
    }
}

pub fn create_pipe(id: &str) -> anyhow::Result<(String, PipeEnd)> {
    let path = fifo::create("audio", id)?;
    Ok((path.to_string_lossy().into_owned(), PipeEnd(path)))
}

pub fn unblock_pipe(name: &str) {
    fifo::unblock(Path::new(name));
}

pub fn remove_pipe(name: &str) {
    fifo::remove(Path::new(name));
}
