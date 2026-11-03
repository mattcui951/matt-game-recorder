//! Thin wrappers over the Win32 calls the recorder needs.

use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetClientRect, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// A `Command` that never flashes a console window.
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Like `hidden_command`, at below-normal CPU priority (long background work such as imports).
pub fn hidden_command_low(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    cmd
}

#[derive(Clone, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
}

/// Snapshot of running processes (cheap: one ToolHelp snapshot).
pub fn processes() -> Vec<ProcInfo> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                out.push(ProcInfo { pid: entry.th32ProcessID, name: String::from_utf16_lossy(&entry.szExeFile[..len]) });
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

pub fn find_process(procs: &[ProcInfo], name: &str) -> Option<u32> {
    procs.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.pid)
}

/// Full path of a running process's executable.
pub fn process_path(pid: u32) -> Option<std::path::PathBuf> {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]).into())
    }
}

struct FindCtx {
    pid: u32,
    class: Option<&'static str>,
    best: Option<(isize, i64)>,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut FindCtx);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid != ctx.pid || !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    if let Some(want) = ctx.class {
        let mut buf = [0u16; 128];
        let n = GetClassNameW(hwnd, &mut buf) as usize;
        if String::from_utf16_lossy(&buf[..n]) != want {
            return BOOL(1);
        }
    }
    let mut r = RECT::default();
    let _ = GetWindowRect(hwnd, &mut r);
    let area = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
    if ctx.best.is_none_or(|(_, a)| area > a) {
        ctx.best = Some((hwnd.0 as isize, area));
    }
    BOOL(1)
}

/// Largest visible top-level window owned by `pid` (optionally of a given class).
pub fn main_window_of(pid: u32, class: Option<&'static str>) -> Option<isize> {
    let mut ctx = FindCtx { pid, class, best: None };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.best.map(|(h, _)| h)
}

/// Client-area size (what window capture records, without the frame).
pub fn window_size(hwnd: isize) -> Option<(u32, u32)> {
    let mut r = RECT::default();
    unsafe { GetClientRect(HWND(hwnd as *mut _), &mut r).ok()? };
    let (w, h) = ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32);
    (w > 0 && h > 0).then_some((w, h))
}

pub fn is_minimized(hwnd: isize) -> bool {
    unsafe { IsIconic(HWND(hwnd as *mut _)).as_bool() }
}

pub fn foreground_window() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

pub fn monitor_of_window(hwnd: isize) -> isize {
    unsafe { MonitorFromWindow(HWND(hwnd as *mut _), MONITOR_DEFAULTTONEAREST).0 as isize }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub index: u32,
    pub handle: isize,
    pub name: String,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

unsafe extern "system" fn monitor_cb(hmon: HMONITOR, _: HDC, _: *mut RECT, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<Monitor>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        let r = info.monitorInfo.rcMonitor;
        let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        list.push(Monitor {
            index: list.len() as u32,
            handle: hmon.0 as isize,
            name: String::from_utf16_lossy(&info.szDevice[..len]).trim_start_matches(r"\\.\").to_string(),
            primary: info.monitorInfo.dwFlags & 1 == 1,
            x: r.left,
            y: r.top,
            width: (r.right - r.left) as u32,
            height: (r.bottom - r.top) as u32,
        });
    }
    BOOL(1)
}

pub fn monitors() -> Vec<Monitor> {
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(monitor_cb), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

/// Suspend or resume every thread of a process (pausing a long ffmpeg job in place).
pub fn suspend_process(pid: u32, suspend: bool) -> bool {
    use windows::core::{s, w};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SUSPEND_RESUME};
    type NtProcessFn = unsafe extern "system" fn(windows::Win32::Foundation::HANDLE) -> i32;
    unsafe {
        let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) else { return false };
        let Some(f) = GetProcAddress(ntdll, if suspend { s!("NtSuspendProcess") } else { s!("NtResumeProcess") }) else { return false };
        let f: NtProcessFn = std::mem::transmute(f);
        let Ok(h) = OpenProcess(PROCESS_SUSPEND_RESUME, false, pid) else { return false };
        let ok = f(h) >= 0;
        let _ = CloseHandle(h);
        ok
    }
}

/// Hide (or show) a window from screen capture.
pub fn exclude_from_capture(hwnd: isize, exclude: bool) {
    unsafe {
        let _ = SetWindowDisplayAffinity(HWND(hwnd as *mut _), if exclude { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE });
    }
}

/// (free bytes available to the user, total bytes) for the volume holding `path`.
/// Installed font family names, sorted. Old bitmap fonts and vertical "@" variants are left out.
pub fn font_families() -> Vec<String> {
    use std::collections::BTreeSet;
    use windows::Win32::Graphics::Gdi::{EnumFontFamiliesExW, GetDC, ReleaseDC, DEFAULT_CHARSET, LOGFONTW, TEXTMETRICW};
    unsafe extern "system" fn add(lf: *const LOGFONTW, _tm: *const TEXTMETRICW, kind: u32, set: LPARAM) -> i32 {
        const RASTER_FONTTYPE: u32 = 1;
        if kind & RASTER_FONTTYPE != 0 {
            return 1;
        }
        let (set, face) = unsafe { (&mut *(set.0 as *mut BTreeSet<String>), &(*lf).lfFaceName) };
        let len = face.iter().position(|&c| c == 0).unwrap_or(face.len());
        let name = String::from_utf16_lossy(&face[..len]);
        if !name.is_empty() && !name.starts_with('@') {
            set.insert(name);
        }
        1
    }
    let mut set: BTreeSet<String> = BTreeSet::new();
    unsafe {
        let hdc = GetDC(None);
        let lf = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
        EnumFontFamiliesExW(hdc, &lf, Some(add), LPARAM(&mut set as *mut _ as isize), 0);
        ReleaseDC(None, hdc);
    }
    set.into_iter().collect()
}

pub fn disk_space(path: &std::path::Path) -> Option<(u64, u64)> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    let wide: Vec<u16> = probe.as_os_str().encode_wide_null();
    let (mut free, mut total) = (0u64, 0u64);
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), Some(&mut total), None).ok()? };
    Some((free, total))
}

/// The volume a path is on ("C:\\", or a mount point), lowercased; `None` if unknown.
pub fn volume_of(path: &std::path::Path) -> Option<String> {
    use windows::Win32::Storage::FileSystem::GetVolumePathNameW;
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    let wide: Vec<u16> = probe.as_os_str().encode_wide_null();
    let mut buf = [0u16; 512];
    unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut buf).ok()? };
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).to_lowercase())
}

/// Both paths are on the same volume, so a file moves between them by renaming (instantly).
pub fn same_volume(a: &std::path::Path, b: &std::path::Path) -> bool {
    matches!((volume_of(a), volume_of(b)), (Some(x), Some(y)) if x == y)
}

trait EncodeWideNull {
    fn encode_wide_null(&self) -> Vec<u16>;
}

impl EncodeWideNull for std::ffi::OsStr {
    fn encode_wide_null(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(std::iter::once(0)).collect()
    }
}

/// Open the file's folder in Explorer with the file selected (or just the folder when the file
/// is gone). Asks the shell directly: `explorer /select,<path>` needs exact quoting and silently
/// opens Documents instead when a path with spaces arrives quoted as one argument.
pub fn reveal_in_explorer(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};
    let target = if path.exists() { path } else { path.parent().filter(|d| d.exists()).ok_or_else(|| anyhow::anyhow!("{} no longer exists", path.display()))? };
    let wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        // The shell call needs COM on this thread; an already initialized thread is fine.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let pidl = ILCreateFromPathW(PCWSTR(wide.as_ptr()));
        if pidl.is_null() {
            anyhow::bail!("Windows could not find {}", target.display());
        }
        let shown = SHOpenFolderAndSelectItems(pidl, None, 0);
        ILFree(Some(pidl));
        shown?;
    }
    Ok(())
}

