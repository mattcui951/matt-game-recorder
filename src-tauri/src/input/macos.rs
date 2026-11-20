//! A listen-only Quartz event tap on its own run loop thread. Listen-only taps get a copy of
//! each event and cannot hold it up, so the game never waits on the recorder. macOS asks for
//! Input Monitoring permission the first time; without it the tap is not created and APM stays 0.

use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRetained, CFRunLoop};
use objc2_core_graphics::{CGEvent, CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy, CGEventType};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// macOS virtual key code for a key name like "R", "Q", "1", "F5" (kVK_* in Carbon's Events.h).
pub fn key_from_name(name: &str) -> u32 {
    const LETTERS: [u32; 26] = [
        0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E, // A-M
        0x2D, 0x1F, 0x23, 0x0C, 0x0F, 0x01, 0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06, // N-Z
    ];
    const DIGITS: [u32; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];
    const FKEYS: [u32; 20] = [
        0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A, 0x40, 0x4F, 0x50, 0x5A,
    ];
    let n = name.trim().to_uppercase();
    if n.len() == 1 {
        let c = n.as_bytes()[0];
        if c.is_ascii_uppercase() {
            return LETTERS[(c - b'A') as usize];
        }
        if c.is_ascii_digit() {
            return DIGITS[(c - b'0') as usize];
        }
    }
    if let Some(f) = n.strip_prefix('F').and_then(|d| d.parse::<usize>().ok()) {
        if (1..=20).contains(&f) {
            return FKEYS[f - 1];
        }
    }
    LETTERS[(b'R' - b'A') as usize]
}

thread_local! {
    /// (game window was frontmost, when that was checked). Asking the window server costs about
    /// a millisecond, so one answer serves every event for a quarter second.
    static FOCUS: Cell<(bool, Option<Instant>)> = const { Cell::new((false, None)) };
}

fn game_focused() -> bool {
    FOCUS.with(|f| {
        let (focused, at) = f.get();
        if at.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
            return focused;
        }
        let g = super::game_window();
        let focused = g != 0 && crate::platform::foreground_window() == g;
        f.set((focused, Some(Instant::now())));
        focused
    })
}

/// The tap, so the callback can turn it back on when macOS disables it.
struct TapRef(*const CFMachPort);

unsafe extern "C-unwind" fn callback(_proxy: CGEventTapProxy, kind: CGEventType, event: NonNull<CGEvent>, user: *mut c_void) -> *mut CGEvent {
    let event_ref = unsafe { event.as_ref() };
    match kind {
        CGEventType::KeyDown | CGEventType::KeyUp => {
            let key = CGEvent::integer_value_field(Some(event_ref), CGEventField::KeyboardEventKeycode);
            super::on_key(key as usize, kind == CGEventType::KeyDown, game_focused);
        }
        CGEventType::LeftMouseDown | CGEventType::RightMouseDown | CGEventType::OtherMouseDown => super::on_clicks(1, game_focused),
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            let tap = unsafe { &*(user as *const TapRef) };
            if !tap.0.is_null() {
                CGEvent::tap_enable(unsafe { &*tap.0 }, true);
            }
        }
        _ => {}
    }
    event.as_ptr()
}

struct RunLoop(CFRetained<CFRunLoop>);
// SAFETY: CFRunLoopStop may be called from any thread; nothing else is done with it off its thread.
unsafe impl Send for RunLoop {}

pub struct Hook {
    run_loop: Option<RunLoop>,
    handle: Option<JoinHandle<()>>,
}

fn mask(kinds: &[CGEventType]) -> u64 {
    kinds.iter().fold(0, |m, k| m | (1u64 << k.0))
}

impl Hook {
    pub fn start(apm: bool) -> Hook {
        if !crate::platform::input_monitoring_allowed() {
            crate::platform::request_input_monitoring();
        }
        let (tx, rx) = std::sync::mpsc::channel::<Option<RunLoop>>();
        let handle = std::thread::Builder::new()
            .name("input".into())
            .spawn(move || {
                let mut kinds = vec![CGEventType::KeyDown, CGEventType::KeyUp];
                if apm {
                    kinds.extend([CGEventType::LeftMouseDown, CGEventType::RightMouseDown, CGEventType::OtherMouseDown]);
                }
                let tap_ref = Box::into_raw(Box::new(TapRef(std::ptr::null())));
                let tap = unsafe {
                    CGEvent::tap_create(
                        CGEventTapLocation::SessionEventTap,
                        CGEventTapPlacement::TailAppendEventTap,
                        CGEventTapOptions::ListenOnly,
                        mask(&kinds),
                        Some(callback),
                        tap_ref.cast(),
                    )
                };
                let (Some(tap), Some(run_loop)) = (tap, CFRunLoop::current()) else {
                    log::warn!("APM: no event tap (Input Monitoring permission missing?)");
                    let _ = tx.send(None);
                    drop(unsafe { Box::from_raw(tap_ref) });
                    return;
                };
                unsafe { (*tap_ref).0 = &*tap };
                let Some(source) = CFMachPort::new_run_loop_source(None, Some(&tap), 0) else {
                    let _ = tx.send(None);
                    return;
                };
                run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
                CGEvent::tap_enable(&tap, true);
                let _ = tx.send(Some(RunLoop(run_loop.clone())));
                CFRunLoop::run();
                CGEvent::tap_enable(&tap, false);
                drop(source);
                drop(tap);
                drop(unsafe { Box::from_raw(tap_ref) });
            })
            .ok();
        let run_loop = rx.recv().ok().flatten();
        Hook { run_loop, handle }
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        let Some(h) = self.handle.take() else { return };
        if let Some(rl) = self.run_loop.take() {
            // A stop that lands before the thread entered its run loop is lost; repeat it.
            while !h.is_finished() {
                rl.0.stop();
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = h.join();
    }
}
