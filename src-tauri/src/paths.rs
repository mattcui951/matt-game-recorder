//! Locations of bundled tools and app data.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_resource_dir(dir: PathBuf) {
    let _ = RESOURCE_DIR.set(dir);
}

pub fn config_dir() -> PathBuf {
    let dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("Matt Game Recorder");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn find_tool(name: &str) -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = RESOURCE_DIR.get() {
        candidates.push(dir.join(name));
        candidates.push(dir.join("binaries").join(name));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(name));
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries").join(name));
    candidates.into_iter().find(|p| p.exists()).unwrap_or_else(|| PathBuf::from(name))
}

pub fn ffmpeg() -> &'static Path {
    static P: OnceLock<PathBuf> = OnceLock::new();
    P.get_or_init(|| find_tool("ffmpeg.exe"))
}

pub fn ffprobe() -> &'static Path {
    static P: OnceLock<PathBuf> = OnceLock::new();
    P.get_or_init(|| find_tool("ffprobe.exe"))
}

/// Make a string safe to use as a Windows file name.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() { "Untitled".into() } else { trimmed }
}

/// Return `dir/stem.ext`, adding " (2)", " (3)"... when the file already exists.
pub fn unique_file(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let stem = sanitize(stem);
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem} ({n}).{ext}"))).find(|p| !p.exists()).unwrap()
}
