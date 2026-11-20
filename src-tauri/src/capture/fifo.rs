//! FIFOs (named pipes on macOS) that carry raw audio and video to ffmpeg. Opening one for
//! writing blocks until ffmpeg opens it for reading, like a Windows named pipe's connect.

use std::fs::File;
use std::path::{Path, PathBuf};

/// Create a FIFO in the temp folder; `kind` and `id` name it.
pub fn create(kind: &str, id: &str) -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("mgr-{kind}-{id}.fifo"));
    let _ = std::fs::remove_file(&path);
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
        anyhow::bail!("could not create {}: {}", path.display(), std::io::Error::last_os_error());
    }
    Ok(path)
}

/// Blocks until a reader opens the FIFO (ffmpeg, or `unblock`).
pub fn open_writer(path: &Path) -> Option<File> {
    std::fs::OpenOptions::new().write(true).open(path).ok()
}

/// Wake a writer still waiting in `open_writer`: a non-blocking open for reading completes its
/// open at once. Its writes then fail, which ends it.
pub fn unblock(path: &Path) {
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path);
}

pub fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}
