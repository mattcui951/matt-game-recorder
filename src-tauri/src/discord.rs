//! Discord rich presence over Discord's local IPC pipe. Needs a Discord application id,
//! which the user creates at discord.com/developers and pastes into Settings.

use serde_json::json;
use std::fs::File;
use std::io::{Read, Write};

pub struct Presence {
    pipe: File,
}

fn frame(op: u32, payload: &serde_json::Value) -> Vec<u8> {
    let body = payload.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn read_frame(pipe: &mut File) -> Option<()> {
    let mut hdr = [0u8; 8];
    pipe.read_exact(&mut hdr).ok()?;
    let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
    let mut body = vec![0u8; len.min(1 << 20)];
    pipe.read_exact(&mut body).ok()
}

impl Presence {
    pub fn start(app_id: &str, state: &str) -> Option<Presence> {
        let mut pipe = (0..10).find_map(|i| File::options().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}")).ok())?;
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
