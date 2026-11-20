//! What the recorder needs from the operating system, with one implementation per platform:
//! processes, windows, displays, disks, fonts and the file manager.
//!
//! Window and display handles are plain integers: an `HWND` / `HMONITOR` on Windows, a
//! `CGWindowID` / `CGDirectDisplayID` on macOS. Only this module and the capture backends look
//! inside them.

use serde::Serialize;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

/// "windows" or "macos": the frontend adapts labels and the title bar to it.
pub const NAME: &str = if cfg!(target_os = "macos") { "macos" } else { "windows" };

#[derive(Clone, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
}

pub fn find_process(procs: &[ProcInfo], name: &str) -> Option<u32> {
    procs.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.pid)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub index: u32,
    pub handle: isize,
    pub name: String,
    pub primary: bool,
    /// Position and size in physical pixels.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Both paths are on the same volume, so a file moves between them by renaming (instantly).
pub fn same_volume(a: &std::path::Path, b: &std::path::Path) -> bool {
    matches!((volume_of(a), volume_of(b)), (Some(x), Some(y)) if x == y)
}

/// The nearest existing ancestor of `path` (itself if it exists).
fn existing_ancestor(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    Some(probe)
}
