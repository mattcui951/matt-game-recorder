//! Discord rich presence over Discord's local IPC channel (a named pipe on Windows, a Unix
//! socket in the temp folder on macOS). Needs a Discord application id, which the user creates
//! at discord.com/developers and pastes into Settings.

use serde_json::json;
use std::io::{Read, Write};

#[cfg(windows)]
type Pipe = std::fs::File;
#[cfg(unix)]
type Pipe = std::os::unix::net::UnixStream;

pub struct Presence {
    pipe: Pipe,
}

#[cfg(windows)]
fn connect(i: u32) -> Option<Pipe> {
    std::fs::File::options().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}")).ok()
}

#[cfg(unix)]
fn connect(i: u32) -> Option<Pipe> {
    // Discord puts its socket in $TMPDIR (a per-user folder on macOS).
    let dir = std::env::var_os("TMPDIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let pipe = Pipe::connect(dir.join(format!("discord-ipc-{i}"))).ok()?;
    let _ = pipe.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    Some(pipe)
}

fn frame(op: u32, payload: &serde_json::Value) -> Vec<u8> {
    let body = payload.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn read_frame(pipe: &mut Pipe) -> Option<()> {
    let mut hdr = [0u8; 8];
    pipe.read_exact(&mut hdr).ok()?;
    let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
    let mut body = vec![0u8; len.min(1 << 20)];
    pipe.read_exact(&mut body).ok()
}

impl Presence {
    pub fn start(app_id: &str, state: &str) -> Option<Presence> {
        let mut pipe = (0..10).find_map(connect)?;
        pipe.write_all(&frame(0, &json!({ "v": 1, "client_id": app_id }))).ok()?;
        read_frame(&mut pipe)?;
        let start = chrono::Utc::now().timestamp();
        let activity = json!({
            "cmd": "SET_ACTIVITY",
            "nonce": uuid::Uuid::new_v4().to_string(),
            "args": {
                "pid": std::process::id(),
                "activity": {
                    "details": "Playing League of Legends",
                    "state": state,
                    "timestamps": { "start": start }
                }
            }
        });
        pipe.write_all(&frame(1, &activity)).ok()?;
        read_frame(&mut pipe)?;
        Some(Presence { pipe })
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        let clear = json!({ "cmd": "SET_ACTIVITY", "nonce": uuid::Uuid::new_v4().to_string(), "args": { "pid": std::process::id() } });
        let _ = self.pipe.write_all(&frame(1, &clear));
    }
}
