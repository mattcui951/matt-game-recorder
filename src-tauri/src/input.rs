//! Low-level keyboard/mouse hooks, active only while a match is recorded.
//! They count actions (for APM) and ultimate-key presses, but only while the game window is
//! in the foreground. The callbacks do a couple of atomic operations and return, so they add
//! no measurable input latency.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::thread::JoinHandle;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_QUIT,
    WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN,
};

static ACTIONS: AtomicU32 = AtomicU32::new(0);
static ULT_PRESSES: AtomicU32 = AtomicU32::new(0);
static GAME_HWND: AtomicIsize = AtomicIsize::new(0);
static ULT_VK: AtomicU32 = AtomicU32::new(0x52); // 'R'
static KEY_DOWN: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];

fn game_focused() -> bool {
    let g = GAME_HWND.load(Ordering::Relaxed);
    g != 0 && unsafe { GetForegroundWindow().0 as isize } == g
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = (kb.vkCode & 0xff) as usize;
        let msg = wparam.0 as u32;
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            // Ignore auto-repeat: only the first down of a held key counts.
            if !KEY_DOWN[vk].swap(true, Ordering::Relaxed) && game_focused() {
                ACTIONS.fetch_add(1, Ordering::Relaxed);
                if vk as u32 == ULT_VK.load(Ordering::Relaxed) {
                    ULT_PRESSES.fetch_add(1, Ordering::Relaxed);
                }
            }
        } else if msg == WM_KEYUP || msg == WM_SYSKEYUP {
            KEY_DOWN[vk].store(false, Ordering::Relaxed);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN) && game_focused() {
            ACTIONS.fetch_add(1, Ordering::Relaxed);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

pub struct InputMonitor {
    thread_id: u32,
    handle: Option<JoinHandle<()>>,
}

/// Virtual-key code for a key name like "R", "Q", "1", "F5".
pub fn vk_from_name(name: &str) -> u32 {
    let n = name.trim().to_uppercase();
    if n.len() == 1 {
        let c = n.as_bytes()[0];
        if c.is_ascii_alphanumeric() {
            return c as u32;
        }
    }
    if let Some(f) = n.strip_prefix('F').and_then(|d| d.parse::<u32>().ok()) {
        if (1..=24).contains(&f) {
            return 0x70 + f - 1;
        }
    }
    0x52
}

impl InputMonitor {
    pub fn start(game_hwnd: isize, ult_key: &str) -> InputMonitor {
        GAME_HWND.store(game_hwnd, Ordering::Relaxed);
        ULT_VK.store(vk_from_name(ult_key), Ordering::Relaxed);
        ACTIONS.store(0, Ordering::Relaxed);
        ULT_PRESSES.store(0, Ordering::Relaxed);
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("input-hooks".into())
            .spawn(move || unsafe {
                let _ = tx.send(GetCurrentThreadId());
                let module = GetModuleHandleW(None).ok();
                let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module.map(Into::into), 0);
                let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module.map(Into::into), 0);
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_QUIT {
                        break;
                    }
                }
                if let Ok(h) = kb {
                    let _ = UnhookWindowsHookEx(h);
                }
                if let Ok(h) = ms {
                    let _ = UnhookWindowsHookEx(h);
                }
            })
            .ok();
        let thread_id = rx.recv().unwrap_or(0);
        InputMonitor { thread_id, handle }
    }

    pub fn set_game_window(&self, hwnd: isize) {
        GAME_HWND.store(hwnd, Ordering::Relaxed);
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
        GAME_HWND.store(0, Ordering::Relaxed);
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
