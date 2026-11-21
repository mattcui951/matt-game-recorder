//! Screen capture on macOS with ScreenCaptureKit (macOS 13+).
//!
//! The video stream delivers NV12 frames, already scaled to the output size on the GPU. A writer
//! thread clocked by wall time hands ffmpeg exactly `fps` raw frames a second through a FIFO,
//! repeating the latest frame while the screen is still (ScreenCaptureKit only sends a frame
//! when something changed). Like the audio mixer, this keeps the video exactly as long as the
//! time that passed, so audio and video stay aligned for the whole match.
//!
//! The system sound has a stream of its own, with its own content filter: the game alone, or
//! everything but this app. ScreenCaptureKit leaves our windows out of a display recording when
//! asked (the Windows display-affinity flag has no effect on it).

use super::audio::{push_samples, Resampler, Ring, RATE};
use super::fifo;
use crate::platform::bridge;
use anyhow::{bail, Context, Result};
use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, Message};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment, CMAudioFormatDescriptionGetStreamBasicDescription, CMBlockBuffer, CMSampleBuffer,
    CMTime, CMTimeFlags,
};
use objc2_core_video::{
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCFrameStatus, SCRunningApplication, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCStreamFrameInfoStatus, SCStreamOutput, SCStreamOutputType, SCWindow,
};
use parking_lot::Mutex;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What to capture: a window (`CGWindowID`) or a display (`CGDirectDisplayID`).
#[derive(Clone, Copy, Debug)]
pub enum Source {
    Window(u32),
    Display(u32),
}

#[derive(Clone, Debug)]
pub struct VideoOptions {
    pub source: Source,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub cursor: bool,
    /// Fit with bars (false) or stretch (true) when the aspect ratio differs.
    pub stretch: bool,
    /// Leave this app's windows out of a display recording.
    pub exclude_self: bool,
}

/// Where a stream's system sound goes, and the resampler (with its source rate) when the
/// sound does not arrive at 48 kHz.
struct AudioSink {
    ring: Ring,
    resampler: Mutex<Option<(u32, Resampler)>>,
}

/// What one stream's output object does with the samples it gets.
struct Shared {
    /// Video: (width, height) frames must have; the latest complete frame, packed NV12.
    size: Option<(usize, usize)>,
    latest: Mutex<Option<Arc<Vec<u8>>>>,
    audio: Option<AudioSink>,
    warned: AtomicBool,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and `StreamOutput` does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "MGRStreamOutput"]
    #[ivars = Arc<Shared>]
    struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(&self, _stream: &SCStream, sample: &CMSampleBuffer, kind: SCStreamOutputType) {
            let shared = self.ivars();
            if kind == SCStreamOutputType::Screen && shared.size.is_some() {
                on_video(shared, sample);
            } else if kind == SCStreamOutputType::Audio {
                on_audio(shared, sample);
            }
        }
    }

    unsafe impl SCStreamDelegate for StreamOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, error: &NSError) {
            // The captured window closed (the game ending), the display went away, or permission
            // was withdrawn. The writer keeps the timeline going with the last frame until the
            // recording is stopped, so the file stays whole.
            log::warn!("capture stream stopped: {}", error.localizedDescription());
        }
    }
);

impl StreamOutput {
    fn new(shared: Arc<Shared>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(shared);
        unsafe { msg_send![super(this), init] }
    }
}

fn on_video(shared: &Shared, sample: &CMSampleBuffer) {
    if !frame_complete(sample) {
        return;
    }
    let Some(image) = (unsafe { sample.image_buffer() }) else { return };
    let (w, h) = shared.size.unwrap_or_default();
    if CVPixelBufferGetWidth(&image) != w || CVPixelBufferGetHeight(&image) != h || CVPixelBufferGetPlaneCount(&image) != 2 {
        if !shared.warned.swap(true, Ordering::Relaxed) {
            log::warn!("capture frame is {}x{}, expected {w}x{h}; skipping such frames", CVPixelBufferGetWidth(&image), CVPixelBufferGetHeight(&image));
        }
        return;
    }
    let mut frame = Vec::with_capacity(w * h * 3 / 2);
    unsafe {
        if CVPixelBufferLockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return;
        }
        // Plane 0 is luma (h rows of w bytes), plane 1 interleaved chroma (h/2 rows of w bytes).
        for plane in 0..2 {
            let base = CVPixelBufferGetBaseAddressOfPlane(&image, plane) as *const u8;
            let stride = CVPixelBufferGetBytesPerRowOfPlane(&image, plane);
            let rows = CVPixelBufferGetHeightOfPlane(&image, plane);
            if base.is_null() || stride < w {
                break;
            }
            for row in 0..rows {
                frame.extend_from_slice(std::slice::from_raw_parts(base.add(row * stride), w));
            }
        }
        CVPixelBufferUnlockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly);
    }
    if frame.len() == w * h * 3 / 2 {
        *shared.latest.lock() = Some(Arc::new(frame));
    }
}

/// Idle, blank and suspended samples carry no new picture.
fn frame_complete(sample: &CMSampleBuffer) -> bool {
    let Some(attachments) = (unsafe { sample.sample_attachments_array(false) }) else { return false };
    let list: &NSArray<NSDictionary<NSString, AnyObject>> = unsafe { bridge(&attachments) };
    let Some(info) = list.firstObject() else { return false };
    let key = unsafe { SCStreamFrameInfoStatus };
    info.objectForKey(key).and_then(|v| v.downcast::<NSNumber>().ok()).is_some_and(|n| n.as_isize() == SCFrameStatus::Complete.0)
}

/// Room for up to eight channels of non-interleaved audio.
#[repr(C)]
struct BufferList {
    count: u32,
    buffers: [AudioBuffer; 8],
}


fn on_audio(shared: &Shared, sample: &CMSampleBuffer) {
    let Some(AudioSink { ring, resampler }) = &shared.audio else { return };
    let Some(desc) = (unsafe { sample.format_description() }) else { return };
    let asbd = unsafe { CMAudioFormatDescriptionGetStreamBasicDescription(&desc) };
    if asbd.is_null() {
        return;
    }
    let asbd = unsafe { &*asbd };
    const FLOAT: u32 = 1 << 0;
    const NON_INTERLEAVED: u32 = 1 << 5;
    if asbd.mFormatFlags & FLOAT == 0 || asbd.mBitsPerChannel != 32 {
        if !shared.warned.swap(true, Ordering::Relaxed) {
            log::warn!("system audio arrives in an unexpected format (flags {:#x}, {} bits)", asbd.mFormatFlags, asbd.mBitsPerChannel);
        }
        return;
    }
    let channels = asbd.mChannelsPerFrame.max(1) as usize;
    let mut list = BufferList { count: 0, buffers: [AudioBuffer { mNumberChannels: 0, mDataByteSize: 0, mData: std::ptr::null_mut() }; 8] };
    let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
    let mut needed = 0;
    // Ask CoreMedia for the exact list size. Passing the capacity of our larger stack buffer
    // returns ArrayTooSmall on macOS 26 even when a stereo list only needs 40 bytes.
    let status = unsafe {
        sample.audio_buffer_list_with_retained_block_buffer(
            &mut needed, std::ptr::null_mut(), 0, None, None, 0, std::ptr::null_mut(),
        )
    };
    if status != 0 || needed > std::mem::size_of::<BufferList>() || needed < std::mem::size_of::<AudioBufferList>() {
        if !shared.warned.swap(true, Ordering::Relaxed) {
            log::warn!("cannot size system audio buffers: status {status}, size {needed}");
        }
        return;
    }
    let status = unsafe {
        sample.audio_buffer_list_with_retained_block_buffer(
            &mut needed,
            (&mut list as *mut BufferList).cast::<AudioBufferList>(),
            needed,
            None,
            None,
            kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
            &mut block,
        )
    };
    if status != 0 || block.is_null() {
        if !shared.warned.swap(true, Ordering::Relaxed) {
            log::warn!("cannot read system audio buffers: status {status}");
        }
        return;
    }
    // Owns the sample memory until we are done reading it.
    let _block = unsafe { CFRetained::from_raw(std::ptr::NonNull::new_unchecked(block)) };
    let buffers = &list.buffers[..(list.count as usize).min(8)];
    let plane = |b: &AudioBuffer| -> &[f32] {
        if b.mData.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(b.mData as *const f32, b.mDataByteSize as usize / 4) } }
    };
    // Interleave the first two channels (or widen mono).
    let interleaved: Vec<f32> = if asbd.mFormatFlags & NON_INTERLEAVED != 0 {
        let left = buffers.first().map(plane).unwrap_or_default();
        let right = buffers.get(1).map(plane).unwrap_or(left);
        left.iter().zip(right).flat_map(|(l, r)| [*l, *r]).collect()
    } else {
        let data = buffers.first().map(plane).unwrap_or_default();
        data.chunks_exact(channels).flat_map(|f| [f[0], *f.get(1).unwrap_or(&f[0])]).collect()
    };
    let rate = asbd.mSampleRate.round() as u32;
    if rate == RATE as u32 {
        push_samples(ring, interleaved);
    } else {
        let mut r = resampler.lock();
        if r.as_ref().is_none_or(|(at, _)| *at != rate) {
            *r = Some((rate, Resampler::new(rate)));
        }
        let mut out = Vec::with_capacity(interleaved.len() * 2);
        if let Some((_, res)) = r.as_mut() {
            res.push(&interleaved, 2, &mut out);
        }
        push_samples(ring, out);
    }
}

// ---------- shareable content ----------

struct Content(Retained<SCShareableContent>);
// SAFETY: SCShareableContent is an immutable snapshot; it is only read after being handed over.
unsafe impl Send for Content {}

/// Windows, displays and apps that can be captured. Fails without Screen Recording permission.
fn shareable_content() -> Result<Retained<SCShareableContent>> {
    // The game loop retries automatically. Querying ScreenCaptureKit while access is denied
    // can show another system prompt on every retry. Preflight here is silent and notices
    // a new grant on the next attempt.
    if !crate::platform::screen_capture_allowed() {
        bail!(permission_hint("Screen Recording permission is required"));
    }
    let (tx, rx) = channel::<Result<Content, String>>();
    let block = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let result = match unsafe { Retained::retain(content) } {
            Some(c) => Ok(Content(c)),
            None => Err(unsafe { error.as_ref() }.map_or_else(|| "no content".to_string(), |e| e.localizedDescription().to_string())),
        };
        let _ = tx.send(result);
    });
    unsafe { SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(true, false, &block) };
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(c)) => Ok(c.0),
        Ok(Err(e)) => bail!(permission_hint(&e)),
        Err(_) => bail!("ScreenCaptureKit did not answer"),
    }
}

fn permission_hint(err: &str) -> String {
    tr!("macOS did not allow screen capture ({err}). Allow Matt Game Recorder in System Settings > Privacy & Security > Screen & System Audio Recording, then try again.",
        zh: "macOS 不允许屏幕录制（{err}）。请在“系统设置 > 隐私与安全性 > 录屏与系统录音”中允许 Matt Game Recorder，然后重试。",
        tw: "macOS 不允許螢幕錄製（{err}）。請在「系統設定 > 隱私權與安全性 > 螢幕與系統錄音」中允許 Matt Game Recorder，然後再試一次。",
        es: "macOS no permitió capturar la pantalla ({err}). Permite Matt Game Recorder en Ajustes del Sistema > Privacidad y seguridad > Grabación de pantalla y audio del sistema y vuelve a intentarlo.",
        fr: "macOS n'a pas autorisé la capture d'écran ({err}). Autorisez Matt Game Recorder dans Réglages Système > Confidentialité et sécurité > Enregistrement de l'écran et de l'audio système, puis réessayez.",
        ja: "macOS が画面収録を許可しませんでした（{err}）。システム設定 > プライバシーとセキュリティ > 画面収録とシステムオーディオ録音 で Matt Game Recorder を許可してから、もう一度お試しください。",
        ko: "macOS가 화면 기록을 허용하지 않았습니다({err}). 시스템 설정 > 개인정보 보호 및 보안 > 화면 및 시스템 오디오 녹음에서 Matt Game Recorder를 허용한 뒤 다시 시도하세요.",
        ru: "macOS не разрешила запись экрана ({err}). Разрешите Matt Game Recorder в Системных настройках > Конфиденциальность и безопасность > Запись экрана и системного звука и попробуйте снова.")
}

fn find_window(content: &SCShareableContent, id: u32) -> Option<Retained<SCWindow>> {
    unsafe { content.windows() }.iter().find(|w| unsafe { w.windowID() } == id)
}

fn find_display(content: &SCShareableContent, id: u32) -> Option<Retained<SCDisplay>> {
    let displays = unsafe { content.displays() };
    displays.iter().find(|d| unsafe { d.displayID() } == id).or_else(|| displays.firstObject())
}

fn find_app(content: &SCShareableContent, pid: u32) -> Option<Retained<SCRunningApplication>> {
    unsafe { content.applications() }.iter().find(|a| unsafe { a.processID() } as u32 == pid)
}

fn time(seconds_den: u32) -> CMTime {
    CMTime { value: 1, timescale: seconds_den.max(1) as i32, flags: CMTimeFlags::Valid, epoch: 0 }
}

fn responds(obj: &NSObject, selector: objc2::runtime::Sel) -> bool {
    obj.respondsToSelector(selector)
}

// ---------- streams ----------

struct Stream {
    stream: Retained<SCStream>,
    _output: Retained<StreamOutput>,
    _queue: DispatchRetained<DispatchQueue>,
}

// SAFETY: SCStream's start/stop methods may be called from any thread.
unsafe impl Send for Stream {}

impl Stream {
    fn start(filter: &SCContentFilter, config: &SCStreamConfiguration, shared: Arc<Shared>, kinds: &[SCStreamOutputType]) -> Result<Stream> {
        let output = StreamOutput::new(shared);
        let delegate = ProtocolObject::<dyn SCStreamDelegate>::from_ref(&*output);
        let stream = unsafe { SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), filter, config, Some(delegate)) };
        let queue = DispatchQueue::new("com.matt.gamerecorder.capture", None);
        let handler = ProtocolObject::<dyn SCStreamOutput>::from_ref(&*output);
        for kind in kinds {
            unsafe { stream.addStreamOutput_type_sampleHandlerQueue_error(handler, *kind, Some(&queue)) }
                .map_err(|e| anyhow::anyhow!("{}", e.localizedDescription()))?;
        }
        let (tx, rx) = channel::<Option<String>>();
        let done = RcBlock::new(move |error: *mut NSError| {
            let _ = tx.send(unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string()));
        });
        unsafe { stream.startCaptureWithCompletionHandler(Some(&done)) };
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(None) => Ok(Stream { stream, _output: output, _queue: queue }),
            Ok(Some(e)) => bail!(permission_hint(&e)),
            Err(_) => bail!("ScreenCaptureKit did not start"),
        }
    }

    fn stop(&self) {
        let (tx, rx) = channel::<()>();
        let done = RcBlock::new(move |_: *mut NSError| {
            let _ = tx.send(());
        });
        unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&done)) };
        let _ = rx.recv_timeout(Duration::from_secs(3));
    }
}

/// A running capture: the video stream and its FIFO writer, and the system-sound stream.
pub struct Capture {
    video: Stream,
    audio: Option<Stream>,
    writer: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    fifo: PathBuf,
}

impl Capture {
    /// Start capturing. ffmpeg must open `fifo_path()` as rawvideo nv12 at `width`x`height` and
    /// `fps`. `audio`: deliver the system sound into this ring, from the app with `game_pid`
    /// only when given.
    pub fn start(opts: &VideoOptions, audio: Option<(Ring, Option<u32>)>, id: &str) -> Result<Capture> {
        let content = shareable_content()?;
        let (w, h) = ((opts.width / 2 * 2).max(2) as usize, (opts.height / 2 * 2).max(2) as usize);
        let me = find_app(&content, std::process::id());
        let none_windows = NSArray::<SCWindow>::new();

        let filter = match opts.source {
            Source::Window(id) => {
                let window = find_window(&content, id).context("the game window is not available to ScreenCaptureKit")?;
                unsafe { SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window) }
            }
            Source::Display(id) => {
                let display = find_display(&content, id).context("no display to capture")?;
                let excluded: Vec<Retained<SCRunningApplication>> = me.iter().filter(|_| opts.exclude_self).cloned().collect();
                let excluded = NSArray::from_retained_slice(&excluded);
                unsafe {
                    SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(SCContentFilter::alloc(), &display, &excluded, &none_windows)
                }
            }
        };
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth(w);
            config.setHeight(h);
            config.setMinimumFrameInterval(time(opts.fps));
            config.setPixelFormat(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
            config.setShowsCursor(opts.cursor);
            config.setQueueDepth(6);
            config.setScalesToFit(true);
            config.setCapturesAudio(false);
            if responds(&config, sel!(setPreservesAspectRatio:)) {
                config.setPreservesAspectRatio(!opts.stretch);
            }
            if responds(&config, sel!(setIgnoreShadowsSingleWindow:)) {
                config.setIgnoreShadowsSingleWindow(true);
            }
        }
        let shared = Arc::new(Shared { size: Some((w, h)), latest: Mutex::new(None), audio: None, warned: AtomicBool::new(false) });
        let video = Stream::start(&filter, &config, shared.clone(), &[SCStreamOutputType::Screen])?;

        let audio = match audio {
            Some((ring, game_pid)) => match start_audio(&content, ring, game_pid, me.as_deref()) {
                Ok(s) => Some(s),
                Err(e) => {
                    video.stop();
                    return Err(e);
                }
            },
            None => None,
        };

        let fifo = match fifo::create("video", id) {
            Ok(p) => p,
            Err(e) => {
                video.stop();
                if let Some(a) = &audio {
                    a.stop();
                }
                return Err(e);
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let (fifo, stop, fps) = (fifo.clone(), stop.clone(), opts.fps.max(1));
            std::thread::Builder::new().name("capture-video".into()).spawn(move || write_frames(&fifo, &shared, fps, (w, h), &stop))?
        };
        Ok(Capture { video, audio, writer: Some(writer), stop, fifo })
    }

    pub fn fifo_path(&self) -> &std::path::Path {
        &self.fifo
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.video.stop();
        if let Some(a) = self.audio.take() {
            a.stop();
        }
        self.stop.store(true, Ordering::SeqCst);
        fifo::unblock(&self.fifo);
        if let Some(t) = self.writer.take() {
            let _ = t.join();
        }
        fifo::remove(&self.fifo);
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if self.writer.is_some() {
            self.shutdown();
        }
    }
}

/// The system sound: from the game alone when it is known to ScreenCaptureKit, otherwise from
/// every app but this one.
fn start_audio(content: &SCShareableContent, ring: Ring, game_pid: Option<u32>, me: Option<&SCRunningApplication>) -> Result<Stream> {
    let display = find_display(content, objc2_core_graphics::CGMainDisplayID()).context("no display for the audio stream")?;
    let none_windows = NSArray::<SCWindow>::new();
    let game = game_pid.and_then(|pid| find_app(content, pid));
    let filter = match &game {
        Some(app) => {
            let apps = NSArray::from_retained_slice(std::slice::from_ref(app));
            unsafe { SCContentFilter::initWithDisplay_includingApplications_exceptingWindows(SCContentFilter::alloc(), &display, &apps, &none_windows) }
        }
        None => {
            let excluded: Vec<Retained<SCRunningApplication>> = me.map(|m| m.retain()).into_iter().collect();
            let excluded = NSArray::from_retained_slice(&excluded);
            unsafe { SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(SCContentFilter::alloc(), &display, &excluded, &none_windows) }
        }
    };
    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        // The picture of this stream is never used: keep it as small and rare as allowed.
        config.setWidth(2);
        config.setHeight(2);
        config.setMinimumFrameInterval(time(1));
        config.setQueueDepth(3);
        config.setCapturesAudio(true);
        config.setSampleRate(RATE as isize);
        config.setChannelCount(2);
        config.setExcludesCurrentProcessAudio(true);
    }
    let shared = Arc::new(Shared { size: None, latest: Mutex::new(None), audio: Some(AudioSink { ring, resampler: Mutex::new(None) }), warned: AtomicBool::new(false) });
    // A screen output too, so the unused frames are taken instead of logged as dropped.
    Stream::start(&filter, &config, shared, &[SCStreamOutputType::Screen, SCStreamOutputType::Audio])
}

/// Hand ffmpeg `fps` frames a second, from the moment it opens the FIFO until stopped.
fn write_frames(fifo: &std::path::Path, shared: &Shared, fps: u32, (w, h): (usize, usize), stop: &AtomicBool) {
    let Some(mut out) = fifo::open_writer(fifo) else { return };
    if stop.load(Ordering::Relaxed) {
        return;
    }
    // Black (video range) until the first frame arrives.
    let mut black = vec![16u8; w * h];
    black.resize(w * h * 3 / 2, 128);
    let black = Arc::new(black);
    let start = Instant::now();
    let mut written: u64 = 0;
    while !stop.load(Ordering::Relaxed) {
        let due = (start.elapsed().as_secs_f64() * fps as f64) as u64 + 1;
        while written < due {
            let frame = shared.latest.lock().clone().unwrap_or_else(|| black.clone());
            if out.write_all(&frame).is_err() {
                return; // ffmpeg exited
            }
            written += 1;
        }
        let next = Duration::from_secs_f64(written as f64 / fps as f64);
        std::thread::sleep(next.saturating_sub(start.elapsed()).min(Duration::from_millis(50)));
    }
}

/// A still of a window or display through the system's `screencapture` tool.
pub fn screenshot(source: Source, output: &std::path::Path) -> Result<()> {
    let mut cmd = std::process::Command::new("screencapture");
    cmd.args(["-x", "-o"]);
    match source {
        Source::Window(id) => cmd.arg(format!("-l{id}")),
        Source::Display(id) => {
            // `-D` counts displays from 1 in the active display order.
            let n = crate::platform::monitors().iter().position(|m| m.handle == id as isize).unwrap_or(0) + 1;
            cmd.arg(format!("-D{n}"))
        }
    };
    let out = cmd.arg(output).output()?;
    if !out.status.success() || !output.exists() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_audio_types::AudioStreamBasicDescription;
    use objc2_core_media::{CMAudioFormatDescriptionCreate, CMAudioSampleBufferCreateWithPacketDescriptions};
    use std::ptr::{null, null_mut, NonNull};

    // Real CoreMedia buffers, no recording permission or audio device required. This catches
    // silent sample loss in the native buffer-list API, not just the Rust interleaving logic.
    #[test]
    fn coremedia_planar_stereo_reaches_the_mixer() {
        unsafe {
            let mut asbd = AudioStreamBasicDescription {
                mSampleRate: 48_000.0, mFormatID: u32::from_be_bytes(*b"lpcm"),
                mFormatFlags: 1 | 8 | 32, mBytesPerPacket: 4, mFramesPerPacket: 1,
                mBytesPerFrame: 4, mChannelsPerFrame: 2, mBitsPerChannel: 32, mReserved: 0,
            };
            let mut desc = null();
            assert_eq!(CMAudioFormatDescriptionCreate(None, NonNull::from(&mut asbd), 0, null(), 0, null(), None, NonNull::from(&mut desc)), 0);
            let desc = CFRetained::from_raw(NonNull::new(desc.cast_mut()).unwrap());
            let mut sample = null_mut();
            assert_eq!(CMAudioSampleBufferCreateWithPacketDescriptions(None, None, false, None, null_mut(), &desc, 4,
                CMTime { value: 0, timescale: 48_000, flags: CMTimeFlags::Valid, epoch: 0 }, null(), NonNull::from(&mut sample)), 0);
            let sample = CFRetained::from_raw(NonNull::new(sample).unwrap());
            let mut left = [0.1f32, 0.2, 0.3, 0.4];
            let mut right = [-0.1f32, -0.2, -0.3, -0.4];
            let mut list = BufferList { count: 2, buffers: [AudioBuffer { mNumberChannels: 0, mDataByteSize: 0, mData: null_mut() }; 8] };
            list.buffers[0] = AudioBuffer { mNumberChannels: 1, mDataByteSize: 16, mData: left.as_mut_ptr().cast() };
            list.buffers[1] = AudioBuffer { mNumberChannels: 1, mDataByteSize: 16, mData: right.as_mut_ptr().cast() };
            assert_eq!(sample.set_data_buffer_from_audio_buffer_list(None, None, 0, NonNull::from(&mut list).cast()), 0);
            assert_eq!(sample.set_data_ready(), 0);
            let ring: Ring = Arc::default();
            let shared = Shared { size: None, latest: Mutex::new(None), audio: Some(AudioSink { ring: ring.clone(), resampler: Mutex::new(None) }), warned: AtomicBool::new(false) };
            on_audio(&shared, &sample);
            assert_eq!(ring.lock().iter().copied().collect::<Vec<_>>(), vec![0.1, -0.1, 0.2, -0.2, 0.3, -0.3, 0.4, -0.4]);
        }
    }
}
