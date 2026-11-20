//! Raw Input, not low-level hooks: Windows sends this thread a copy of each input after the
//! fact (RIDEV_INPUTSINK on a message-only window), so the recorder is never in the path from
//! the mouse or keyboard to the game. With WH_*_LL hooks every key, click and mouse movement
//! (about 1000 a second from a gaming mouse) went through the recorder before the game saw it.

use std::thread::JoinHandle;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT,
    RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW, PostThreadMessageW, RegisterClassW,
    HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSW,
};

/// Any mouse button pressed (left, right, middle, 4, 5).
const BUTTON_DOWNS: u16 = 0x0001 | 0x0004 | 0x0010 | 0x0040 | 0x0100;

fn game_focused() -> bool {
    let g = super::game_window();
    g != 0 && unsafe { GetForegroundWindow().0 as isize } == g
}

/// Count what one WM_INPUT carries.
unsafe fn on_raw_input(lparam: LPARAM) {
    let mut data = RAWINPUT::default();
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let got = GetRawInputData(HRAWINPUT(lparam.0 as _), RID_INPUT, Some(&mut data as *mut _ as *mut _), &mut size, std::mem::size_of::<RAWINPUTHEADER>() as u32);
    if got == u32::MAX || got == 0 {
        return;
    }
    if data.header.dwType == RIM_TYPEKEYBOARD.0 {
        let kb = data.data.keyboard;
        match kb.Message {
            WM_KEYDOWN | WM_SYSKEYDOWN => super::on_key(kb.VKey as usize, true, game_focused),
            WM_KEYUP | WM_SYSKEYUP => super::on_key(kb.VKey as usize, false, game_focused),
            _ => {}
        }
    } else if data.header.dwType == RIM_TYPEMOUSE.0 {
        let flags = data.data.mouse.Anonymous.Anonymous.usButtonFlags;
        super::on_clicks((flags & BUTTON_DOWNS).count_ones(), game_focused);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Keyboard (usage 6) and, for APM, mouse (usage 2) on the generic desktop page, to `hwnd` (or
/// removed). Without APM only the ultimate key matters, so mouse movement never reaches us.
fn devices(hwnd: HWND, mouse: bool, remove: bool) -> Vec<RAWINPUTDEVICE> {
    let (flags, target) = if remove { (RIDEV_REMOVE, HWND::default()) } else { (RIDEV_INPUTSINK, hwnd) };
    let usages: &[u16] = if mouse { &[0x06, 0x02] } else { &[0x06] };
    usages.iter().map(|&usage| RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: usage, dwFlags: flags, hwndTarget: target }).collect()
}

/// Virtual-key code for a key name like "R", "Q", "1", "F5".
pub fn key_from_name(name: &str) -> u32 {
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

pub struct Hook {
    thread_id: u32,
    handle: Option<JoinHandle<()>>,
}

impl Hook {
    pub fn start(apm: bool) -> Hook {
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("input".into())
            .spawn(move || unsafe {
                let module = GetModuleHandleW(None).ok();
                let class = w!("MggRawInputSink");
                let wc = WNDCLASSW { lpfnWndProc: Some(wnd_proc), hInstance: module.unwrap_or_default().into(), lpszClassName: class, ..Default::default() };
                RegisterClassW(&wc);
                let window = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, module.map(Into::into), None);
                // The window gave this thread its message queue, so the quit message can't be lost.
                let _ = tx.send(GetCurrentThreadId());
                let Ok(hwnd) = window else {
                    log::warn!("APM: could not create the input window");
                    return;
                };
                let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
                if let Err(e) = RegisterRawInputDevices(&devices(hwnd, apm, false), size) {
                    log::warn!("APM: raw input unavailable: {e}");
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_INPUT {
                        on_raw_input(msg.lParam);
                    }
                    // Lets Windows release the input's buffer.
                    DispatchMessageW(&msg);
                }
                let _ = RegisterRawInputDevices(&devices(hwnd, apm, true), size);
                let _ = DestroyWindow(hwnd);
            })
            .ok();
        let thread_id = rx.recv().unwrap_or(0);
        Hook { thread_id, handle }
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
