//! Keyboard and mouse actions while a match is recorded, for APM and the ultimate-key mark,
//! counted only while the game window is in the foreground. Each platform delivers a copy of the
//! input after the fact (Raw Input on Windows, a listen-only event tap on macOS), so the
//! recorder is never in the path from the mouse or keyboard to the game.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as imp;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use self::macos as imp;

pub use imp::key_from_name;

static ACTIONS: AtomicU32 = AtomicU32::new(0);
static ULT_PRESSES: AtomicU32 = AtomicU32::new(0);
static GAME_WINDOW: AtomicIsize = AtomicIsize::new(0);
static ULT_KEY: AtomicU32 = AtomicU32::new(0);
static KEY_DOWN: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];

fn game_window() -> isize {
    GAME_WINDOW.load(Ordering::Relaxed)
}

/// A key (platform key code) went down or up. Auto-repeat is ignored: only the first down of a
/// held key counts.
fn on_key(key: usize, down: bool, game_focused: impl FnOnce() -> bool) {
    let key = key & 0xff;
    if !down {
        KEY_DOWN[key].store(false, Ordering::Relaxed);
    } else if !KEY_DOWN[key].swap(true, Ordering::Relaxed) && game_focused() {
        ACTIONS.fetch_add(1, Ordering::Relaxed);
        if key as u32 == ULT_KEY.load(Ordering::Relaxed) {
            ULT_PRESSES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// `n` mouse buttons went down.
fn on_clicks(n: u32, game_focused: impl FnOnce() -> bool) {
    if n > 0 && game_focused() {
        ACTIONS.fetch_add(n, Ordering::Relaxed);
    }
}

pub struct InputMonitor {
    _hook: imp::Hook,
}

impl InputMonitor {
    /// `apm`: count every key press and click; otherwise only the ultimate key is watched.
    pub fn start(game_window: isize, ult_key: &str, apm: bool) -> InputMonitor {
        GAME_WINDOW.store(game_window, Ordering::Relaxed);
        ULT_KEY.store(key_from_name(ult_key), Ordering::Relaxed);
        ACTIONS.store(0, Ordering::Relaxed);
        ULT_PRESSES.store(0, Ordering::Relaxed);
        InputMonitor { _hook: imp::Hook::start(apm) }
    }

    pub fn set_game_window(&self, window: isize) {
        GAME_WINDOW.store(window, Ordering::Relaxed);
    }

    /// Actions since the last call.
    pub fn take_actions(&self) -> u32 {
        ACTIONS.swap(0, Ordering::Relaxed)
    }

    /// Ultimate-key presses since the last call.
    pub fn take_ult_presses(&self) -> u32 {
        ULT_PRESSES.swap(0, Ordering::Relaxed)
    }
}

impl Drop for InputMonitor {
    fn drop(&mut self) {
        GAME_WINDOW.store(0, Ordering::Relaxed);
    }
}
