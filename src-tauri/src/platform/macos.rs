//! The platform layer on macOS: libproc for processes, the Quartz window list for windows,
//! CoreGraphics for displays, CoreText for fonts and Finder through `open`.
//!
//! Window handles are `CGWindowID`s and display handles `CGDirectDisplayID`s. Reading window
//! bounds and owners from the window list needs no permission (only titles would).

use super::{existing_ancestor, Monitor, ProcInfo};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFArray, CFString, CGPoint};
use objc2_core_graphics::{
    kCGWindowBounds, kCGWindowIsOnscreen, kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID, CGDirectDisplayID, CGDisplayBounds,
    CGDisplayCopyDisplayMode, CGDisplayIsBuiltin, CGDisplayIsMain, CGDisplayMode, CGGetActiveDisplayList, CGGetDisplaysWithPoint,
    CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A `Command` for a helper process (no console windows to hide on macOS).
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    Command::new(program)
}

/// Like `hidden_command`, at a lower CPU priority (long background work such as imports).
pub fn hidden_command_low(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    // SAFETY: setpriority is async-signal-safe, so it may run between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::setpriority(libc::PRIO_PROCESS, 0, 10);
            Ok(())
        });
    }
    cmd
}

/// Running processes, named by their executable's file name.
pub fn processes() -> Vec<ProcInfo> {
    let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if n <= 0 {
        return vec![];
    }
    // Room for processes started between the two calls.
    let mut pids = vec![0 as libc::pid_t; n as usize + 64];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    let got = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(got.max(0) as usize);
    pids.into_iter()
        .filter(|&pid| pid > 0)
        .filter_map(|pid| {
            let name = process_path(pid as u32).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).or_else(|| process_name(pid))?;
            Some(ProcInfo { pid: pid as u32, name })
        })
        .collect()
}

fn process_name(pid: libc::pid_t) -> Option<String> {
    let mut buf = [0u8; 256];
    let len = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (len > 0).then(|| String::from_utf8_lossy(&buf[..len as usize]).into_owned())
}

/// Full path of a running process's executable.
pub fn process_path(pid: u32) -> Option<PathBuf> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let len = unsafe { libc::proc_pidpath(pid as libc::c_int, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (len > 0).then(|| PathBuf::from(String::from_utf8_lossy(&buf[..len as usize]).into_owned()))
}

// ---------- windows ----------

/// One entry of the Quartz window list, in points (global coordinates, origin top left).
#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub id: u32,
    pub pid: u32,
    pub layer: i64,
    pub onscreen: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A CoreFoundation array seen as the Foundation array it is toll-free bridged to.
pub(crate) unsafe fn bridge<T: objc2::Message>(array: &CFArray) -> &NSArray<T> {
    unsafe { &*(array as *const CFArray as *const NSArray<T>) }
}

fn ns(key: &CFString) -> &NSString {
    unsafe { &*(key as *const CFString as *const NSString) }
}

fn number(dict: &NSDictionary<NSString, AnyObject>, key: &NSString) -> Option<Retained<NSNumber>> {
    dict.objectForKey(key)?.downcast::<NSNumber>().ok()
}

/// Every window, front to back. `onscreen_only` leaves out minimized and hidden ones and those
/// on other Spaces.
pub fn window_list(onscreen_only: bool) -> Vec<WindowInfo> {
    let option = if onscreen_only {
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements
    } else {
        CGWindowListOption::OptionAll | CGWindowListOption::ExcludeDesktopElements
    };
    let Some(list) = CGWindowListCopyWindowInfo(option, 0) else { return vec![] };
    let list: &NSArray<NSDictionary<NSString, AnyObject>> = unsafe { bridge(&list) };
    let (k_id, k_pid, k_layer, k_onscreen, k_bounds) = unsafe {
        (ns(kCGWindowNumber), ns(kCGWindowOwnerPID), ns(kCGWindowLayer), ns(kCGWindowIsOnscreen), ns(kCGWindowBounds))
    };
    let (kx, ky, kw, kh) = (NSString::from_str("X"), NSString::from_str("Y"), NSString::from_str("Width"), NSString::from_str("Height"));
    list.iter()
        .filter_map(|d| {
            let bounds = d.objectForKey(k_bounds)?.downcast::<NSDictionary>().ok()?;
            let bounds: &NSDictionary<NSString, AnyObject> = unsafe { &*(Retained::as_ptr(&bounds) as *const NSDictionary<NSString, AnyObject>) };
            Some(WindowInfo {
                id: number(&d, k_id)?.as_u32(),
                pid: number(&d, k_pid)?.as_i32() as u32,
                layer: number(&d, k_layer).map_or(0, |n| n.as_i64()),
                onscreen: number(&d, k_onscreen).is_some_and(|n| n.as_bool()),
                x: number(bounds, &kx)?.as_f64(),
                y: number(bounds, &ky)?.as_f64(),
                width: number(bounds, &kw)?.as_f64(),
                height: number(bounds, &kh)?.as_f64(),
            })
        })
        .collect()
}

fn window(id: isize) -> Option<WindowInfo> {
    window_list(false).into_iter().find(|w| w.id as isize == id)
}

/// Largest normal window owned by `pid`. `class` is a Windows window class; macOS has none.
pub fn main_window_of(pid: u32, _class: Option<&'static str>) -> Option<isize> {
    window_list(false)
        .into_iter()
        // Layer 0 holds normal windows; menu bar extras and the Dock live far above it.
        .filter(|w| w.pid == pid && w.layer < 25 && w.width >= 1.0 && w.height >= 1.0)
        .max_by(|a, b| (a.onscreen, a.width * a.height).partial_cmp(&(b.onscreen, b.width * b.height)).unwrap_or(std::cmp::Ordering::Equal))
        .map(|w| w.id as isize)
}

/// The window's size in pixels (what capture records).
pub fn window_size(id: isize) -> Option<(u32, u32)> {
    let w = window(id)?;
    let scale = display_scale(display_at(w.x + w.width / 2.0, w.y + w.height / 2.0)?);
    let (pw, ph) = ((w.width * scale).round() as u32, (w.height * scale).round() as u32);
    (pw > 0 && ph > 0).then_some((pw, ph))
}

/// Minimized, hidden, or on another Space: nothing of it can be captured right now.
pub fn is_minimized(id: isize) -> bool {
    window(id).is_none_or(|w| !w.onscreen)
}

/// The frontmost normal window.
pub fn foreground_window() -> isize {
    window_list(true).into_iter().find(|w| w.layer == 0).map_or(0, |w| w.id as isize)
}

pub fn monitor_of_window(id: isize) -> isize {
    window(id).and_then(|w| display_at(w.x + w.width / 2.0, w.y + w.height / 2.0)).unwrap_or_else(main_display) as isize
}

// ---------- displays ----------

fn main_display() -> CGDirectDisplayID {
    objc2_core_graphics::CGMainDisplayID()
}

fn display_at(x: f64, y: f64) -> Option<CGDirectDisplayID> {
    let mut id: CGDirectDisplayID = 0;
    let mut count = 0u32;
    let err = unsafe { CGGetDisplaysWithPoint(CGPoint { x, y }, 1, &mut id, &mut count) };
    (err.0 == 0 && count > 0).then_some(id)
}

/// Pixels per point on a display (2 on Retina screens in their default modes).
pub fn display_scale(id: CGDirectDisplayID) -> f64 {
    let points = CGDisplayBounds(id).size.width;
    let pixels = CGDisplayCopyDisplayMode(id).map_or(0, |m| CGDisplayMode::pixel_width(Some(&m)));
    if points > 0.0 && pixels > 0 { pixels as f64 / points } else { 1.0 }
}

pub fn monitors() -> Vec<Monitor> {
    let mut ids = [0 as CGDirectDisplayID; 16];
    let mut count = 0u32;
    if unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) }.0 != 0 {
        return vec![];
    }
    let mut external = 0;
    ids[..count as usize]
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let b = CGDisplayBounds(id);
            let scale = display_scale(id);
            let name = if CGDisplayIsBuiltin(id) {
                "Built-in Display".to_string()
            } else {
                external += 1;
                format!("Display {external}")
            };
            Monitor {
                index: i as u32,
                handle: id as isize,
                name,
                primary: CGDisplayIsMain(id),
                x: (b.origin.x * scale).round() as i32,
                y: (b.origin.y * scale).round() as i32,
                width: (b.size.width * scale).round() as u32,
                height: (b.size.height * scale).round() as u32,
            }
        })
        .collect()
}

// ---------- processes, windows of ours ----------

/// Pause or resume a process (a long ffmpeg job, in place).
pub fn suspend_process(pid: u32, suspend: bool) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, if suspend { libc::SIGSTOP } else { libc::SIGCONT }) == 0 }
}

/// Hide (or show) one of our windows from screen capture. ScreenCaptureKit leaves our windows out
/// of the recording itself (see `capture::sck`); this covers other capture tools.
pub fn exclude_from_capture(w: &tauri::WebviewWindow, exclude: bool) {
    let _ = w.set_content_protected(exclude);
}

/// Installed font family names, sorted. System-private families (".SF NS ...") are left out.
pub fn font_families() -> Vec<String> {
    let names = unsafe { objc2_core_text::CTFontManagerCopyAvailableFontFamilyNames() };
    let names: &NSArray<NSString> = unsafe { bridge(&names) };
    let mut out: Vec<String> = names.iter().map(|n| n.to_string()).filter(|n| !n.is_empty() && !n.starts_with('.')).collect();
    out.sort();
    out.dedup();
    out
}

// ---------- disks and Finder ----------

/// (free bytes available to the user, total bytes) for the volume holding `path`.
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    let probe = existing_ancestor(path)?;
    let c = std::ffi::CString::new(probe.as_os_str().as_encoded_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let unit = st.f_frsize as u64;
    Some((st.f_bavail as u64 * unit, st.f_blocks as u64 * unit))
}

/// The volume a path is on (its device number); `None` if unknown.
pub fn volume_of(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    Some(std::fs::metadata(existing_ancestor(path)?).ok()?.dev().to_string())
}

/// Show the file in Finder, selected (or just its folder when the file is gone).
pub fn reveal_in_file_manager(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        Command::new("open").arg("-R").arg(path).spawn()?;
        return Ok(());
    }
    let dir = path.parent().filter(|d| d.exists()).ok_or_else(|| anyhow::anyhow!("{} no longer exists", path.display()))?;
    open_folder(dir)
}

/// Open a folder in Finder.
pub fn open_folder(path: &Path) -> anyhow::Result<()> {
    Command::new("open").arg(path).spawn()?;
    Ok(())
}

// ---------- privacy permissions ----------

/// Screen Recording permission (System Settings > Privacy & Security), needed for any capture.
pub fn screen_capture_allowed() -> bool {
    objc2_core_graphics::CGPreflightScreenCaptureAccess()
}

/// Ask for Screen Recording permission. macOS shows its prompt once; later calls only report.
pub fn request_screen_capture() -> bool {
    objc2_core_graphics::CGRequestScreenCaptureAccess()
}

/// Input Monitoring permission, needed to count actions (APM) and see the ultimate key.
pub fn input_monitoring_allowed() -> bool {
    objc2_core_graphics::CGPreflightListenEventAccess()
}

pub fn request_input_monitoring() -> bool {
    objc2_core_graphics::CGRequestListenEventAccess()
}

/// Open the matching page of System Settings > Privacy & Security.
pub fn open_privacy_settings(pane: &str) -> anyhow::Result<()> {
    let anchor = match pane {
        "screen" => "Privacy_ScreenCapture",
        "input" => "Privacy_ListenEvent",
        "microphone" => "Privacy_Microphone",
        "camera" => "Privacy_Camera",
        _ => anyhow::bail!("unknown privacy pane {pane}"),
    };
    Command::new("open").arg(format!("x-apple.systempreferences:com.apple.preference.security?{anchor}")).spawn()?;
    Ok(())
}
