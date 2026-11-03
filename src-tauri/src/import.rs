//! Importing media from elsewhere:
//!   * Outplayed: every video is re-encoded to this app's format (AV1 + Opus, hardware encoder)
//!     and keeps its champion, KDA, result and timeline events, read from Outplayed's own
//!     library database (Chromium IndexedDB inside Overwolf).
//!   * Another Matt Game Recorder library: sessions and recordings are copied as-is.
//!
//! Both imports skip what was already imported, so they can be stopped and resumed.

use crate::core::Core;
use crate::jobs::JobCtx;
use crate::library::{RecEvent, Session, SESSION_FILE};
use crate::lol::Kda;
use crate::paths;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::Serialize;
use serde_json::{Map, Value};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const OUTPLAYED_EXTENSION: &str = "cghphpbjeabdkomiphingnegihoigeggcfphdofo";
const SKIP_DIRS: &[&str] = &["Recently Deleted", "temp-capture", ".cache", "Projects"];
/// Software AV1 encoder the processor uses when it helps the GPU.
const CPU_ENCODER: &str = "libsvtav1";

/// "Use the CPU as a second encoder" for Outplayed imports. Off at every launch on purpose:
/// it slows the whole computer down, so it is never switched on behind the user's back.
static CPU_ASSIST: AtomicBool = AtomicBool::new(false);

pub fn cpu_assist() -> bool {
    CPU_ASSIST.load(Ordering::Relaxed)
}

pub fn set_cpu_assist(on: bool) {
    CPU_ASSIST.store(on, Ordering::Relaxed);
    log::info!("CPU as second encoder {}", if on { "on" } else { "off" });
}

/// Only worth offering when the main encoder is a GPU; otherwise the CPU is already doing the work.
pub fn cpu_assist_available(core: &Core) -> bool {
    crate::ffmpeg::family(&crate::ffmpeg::best_encoder(&core.settings().video.encoder)) != "cpu"
}

static MEASURING: AtomicBool = AtomicBool::new(false);

/// How much faster an import runs with CPU help on this PC: 1 + CPU speed / GPU speed,
/// both measured here (test encodes, then the real conversions).
pub fn cpu_speedup(core: &Core) -> Option<f64> {
    let gpu = crate::ffmpeg::speed(&crate::ffmpeg::best_encoder(&core.settings().video.encoder))?;
    let cpu = crate::ffmpeg::speed(CPU_ENCODER)?;
    Some(1.0 + cpu / gpu)
}

pub fn measuring_speed() -> bool {
    MEASURING.load(Ordering::Relaxed)
}

/// Measure whichever encoder speeds are still unknown, in the background (a few seconds each),
/// then call `done`. Never while a game is being recorded.
pub fn measure_speed(core: &Arc<Core>, done: impl FnOnce() + Send + 'static) {
    if MEASURING.swap(true, Ordering::Relaxed) {
        return;
    }
    let core = core.clone();
    std::thread::spawn(move || {
        let settings = core.settings();
        let gpu = crate::ffmpeg::best_encoder(&settings.video.encoder);
        let busy = || core.status.read().state == "recording";
        let importing = core.jobs.list().iter().any(|j| j.kind == "import" && j.state == "running");
        if let Some(src) = bench_source(&core) {
            // A running import measures the GPU within seconds; a test encode would only compete with it.
            if crate::ffmpeg::speed(&gpu).is_none() && !importing && !busy() {
                if let Err(e) = crate::ffmpeg::benchmark(&gpu, &src, settings.video.bitrate_kbps) {
                    log::warn!("could not measure {gpu}: {e:#}");
                }
            }
            if crate::ffmpeg::speed(CPU_ENCODER).is_none() && !busy() {
                if let Err(e) = crate::ffmpeg::benchmark(CPU_ENCODER, &src, settings.video.bitrate_kbps) {
                    log::warn!("could not measure {CPU_ENCODER}: {e:#}");
                }
            }
            for _ in 0..20 {
                if crate::ffmpeg::speed(&gpu).is_some() || !importing {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        MEASURING.store(false, Ordering::Relaxed);
        done();
    });
}

/// A video like the ones being imported: the biggest in the Outplayed folder, else one of ours.
fn bench_source(core: &Core) -> Option<PathBuf> {
    let folder = pending().map(|p| p.folder).or_else(|| scan_outplayed(core, None).folder.map(PathBuf::from));
    let outplayed = folder.and_then(|f| walk_mp4(&f).into_iter().max_by_key(|f| f.metadata().map(|m| m.len()).unwrap_or(0)));
    outplayed.or_else(|| core.library.snapshot().recordings.into_iter().filter(|r| r.deleted_at.is_none()).map(|r| r.path()).find(|p| p.exists()))
}
/// Extension of a video still being converted by an import.
pub const PARTIAL_EXT: &str = "importing.mp4";

// ---------------------------------------------------------------------------------------------
// LevelDB (read-only): enough to list every key/value of a Chromium IndexedDB folder.

mod leveldb {
    use std::path::Path;

    fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
        let (mut r, mut s) = (0u64, 0);
        loop {
            let c = *b.get(*i)?;
            *i += 1;
            r |= ((c & 0x7f) as u64) << s;
            if c < 0x80 {
                return Some(r);
            }
            s += 7;
            if s > 63 {
                return None;
            }
        }
    }

    fn block(data: &[u8], off: usize, size: usize) -> Option<Vec<u8>> {
        let raw = data.get(off..off + size)?;
        match *data.get(off + size)? {
            1 => snap::raw::Decoder::new().decompress_vec(raw).ok(),
            _ => Some(raw.to_vec()),
        }
    }

    fn entries(blk: &[u8], out: &mut Vec<(Vec<u8>, Vec<u8>)>) -> Option<()> {
        let n = u32::from_le_bytes(blk.get(blk.len().checked_sub(4)?..)?.try_into().ok()?) as usize;
        let end = blk.len().checked_sub(4 + 4 * n)?;
        let mut i = 0;
        let mut key: Vec<u8> = Vec::new();
        while i < end {
            let shared = varint(blk, &mut i)? as usize;
            let non_shared = varint(blk, &mut i)? as usize;
            let vlen = varint(blk, &mut i)? as usize;
            key.truncate(shared);
            key.extend_from_slice(blk.get(i..i + non_shared)?);
            i += non_shared;
            out.push((key.clone(), blk.get(i..i + vlen)?.to_vec()));
            i += vlen;
        }
        Some(())
    }

    fn table(data: &[u8], out: &mut Vec<(Vec<u8>, Vec<u8>)>) -> Option<()> {
        let footer = data.get(data.len().checked_sub(48)?..)?;
        let mut j = 0;
        varint(footer, &mut j)?;
        varint(footer, &mut j)?;
        let ioff = varint(footer, &mut j)? as usize;
        let isize = varint(footer, &mut j)? as usize;
        let mut index = Vec::new();
        entries(&block(data, ioff, isize)?, &mut index)?;
        for (_, handle) in index {
            let mut k = 0;
            let off = varint(&handle, &mut k)? as usize;
            let size = varint(&handle, &mut k)? as usize;
            entries(&block(data, off, size)?, out)?;
        }
        Some(())
    }

    fn log(data: &[u8], out: &mut Vec<(Vec<u8>, Vec<u8>)>) {
        let mut i = 0;
        let mut rec: Vec<u8> = Vec::new();
        while i + 7 <= data.len() {
            let len = u16::from_le_bytes([data[i + 4], data[i + 5]]) as usize;
            let kind = data[i + 6];
            if len == 0 && kind == 0 {
                i = (i / 32768 + 1) * 32768;
                continue;
            }
            let Some(chunk) = data.get(i + 7..i + 7 + len) else { break };
            rec.extend_from_slice(chunk);
            i += 7 + len;
            if kind == 1 || kind == 4 {
                batch(&rec, out);
                rec.clear();
            }
        }
    }

    fn batch(rec: &[u8], out: &mut Vec<(Vec<u8>, Vec<u8>)>) -> Option<()> {
        let count = u32::from_le_bytes(rec.get(8..12)?.try_into().ok()?);
        let mut p = 12;
        for _ in 0..count {
            let kind = *rec.get(p)?;
            p += 1;
            let kl = varint(rec, &mut p)? as usize;
            let k = rec.get(p..p + kl)?.to_vec();
            p += kl;
            if kind == 1 {
                let vl = varint(rec, &mut p)? as usize;
                out.push((k, rec.get(p..p + vl)?.to_vec()));
                p += vl;
            }
        }
        Some(())
    }

    /// All key/value pairs, oldest table first and the log last (later entries win).
    pub fn dump(dir: &Path) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        let mut files: Vec<_> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        files.sort();
        for f in files.iter().filter(|f| f.extension().is_some_and(|x| x == "ldb")) {
            if let Ok(data) = std::fs::read(f) {
                let _ = table(&data, &mut out);
            }
        }
        for f in files.iter().filter(|f| f.extension().is_some_and(|x| x == "log")) {
            if let Ok(data) = std::fs::read(f) {
                log(&data, &mut out);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// V8 ValueSerializer (the format IndexedDB stores objects in), decoded into JSON values.

mod v8 {
    use serde_json::{Map, Number, Value};

    thread_local! {
        /// Position and tag of the last unsupported value (diagnostics).
        pub static UNKNOWN: std::cell::Cell<Option<(usize, u8)>> = const { std::cell::Cell::new(None) };
    }

    struct Reader<'a> {
        b: &'a [u8],
        i: usize,
        ids: Vec<Value>,
    }

    impl Reader<'_> {
        fn byte(&mut self) -> Option<u8> {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            Some(c)
        }

        fn peek(&self) -> Option<u8> {
            self.b.get(self.i).copied()
        }

        fn varint(&mut self) -> Option<u64> {
            let (mut r, mut s) = (0u64, 0);
            loop {
                let c = self.byte()?;
                r |= ((c & 0x7f) as u64) << s;
                if c < 0x80 {
                    return Some(r);
                }
                s += 7;
                if s > 63 {
                    return None;
                }
            }
        }

        fn double(&mut self) -> Option<f64> {
            let v = f64::from_le_bytes(self.b.get(self.i..self.i + 8)?.try_into().ok()?);
            self.i += 8;
            Some(v)
        }

        fn bytes(&mut self, n: usize) -> Option<&[u8]> {
            let s = self.b.get(self.i..self.i + n)?;
            self.i += n;
            Some(s)
        }

        fn num(v: f64) -> Value {
            Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null)
        }

        fn reserve(&mut self) -> usize {
            self.ids.push(Value::Null);
            self.ids.len() - 1
        }

        fn key(&mut self) -> Option<String> {
            Some(match self.value()? {
                Value::String(s) => s,
                Value::Number(n) => n.to_string(),
                other => other.to_string(),
            })
        }

        fn value(&mut self) -> Option<Value> {
            loop {
                let tag = self.byte()?;
                return Some(match tag {
                    0x00 => continue, // padding
                    b'_' | b'0' | b'-' => Value::Null,
                    b'T' => Value::Bool(true),
                    b'F' => Value::Bool(false),
                    b'I' => {
                        let v = self.varint()?;
                        Value::from(((v >> 1) as i64) ^ -((v & 1) as i64))
                    }
                    b'U' => Value::from(self.varint()?),
                    b'N' => Self::num(self.double()?),
                    b'"' => {
                        let n = self.varint()? as usize;
                        Value::String(self.bytes(n)?.iter().map(|&c| c as char).collect())
                    }
                    b'c' => {
                        let n = self.varint()? as usize;
                        let units: Vec<u16> = self.bytes(n)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                        Value::String(String::from_utf16_lossy(&units))
                    }
                    b'S' => {
                        let n = self.varint()? as usize;
                        Value::String(String::from_utf8_lossy(self.bytes(n)?).into_owned())
                    }
                    b'D' | b'n' => {
                        let id = self.reserve();
                        let v = Self::num(self.double()?);
                        self.ids[id] = v.clone();
                        v
                    }
                    b'y' | b'x' => {
                        let id = self.reserve();
                        self.ids[id] = Value::Bool(tag == b'y');
                        Value::Bool(tag == b'y')
                    }
                    b'o' => {
                        let id = self.reserve();
                        let mut m = Map::new();
                        loop {
                            if self.peek()? == b'{' {
                                self.i += 1;
                                self.varint()?;
                                break;
                            }
                            let k = self.key()?;
                            let v = self.value()?;
                            m.insert(k, v);
                        }
                        self.ids[id] = Value::Object(m.clone());
                        Value::Object(m)
                    }
                    b'a' => {
                        let len = self.varint()? as usize;
                        let id = self.reserve();
                        let mut arr = vec![Value::Null; len.min(4096)];
                        loop {
                            if self.peek()? == b'@' {
                                self.i += 1;
                                self.varint()?;
                                self.varint()?;
                                break;
                            }
                            let k = self.value()?;
                            let v = self.value()?;
                            if let Some(slot) = k.as_u64().and_then(|ix| arr.get_mut(ix as usize)) {
                                *slot = v;
                            }
                        }
                        self.ids[id] = Value::Array(arr.clone());
                        Value::Array(arr)
                    }
                    b'A' => {
                        let len = self.varint()? as usize;
                        let id = self.reserve();
                        let mut arr = Vec::with_capacity(len.min(4096));
                        for _ in 0..len {
                            arr.push(self.value()?);
                        }
                        loop {
                            if self.peek()? == b'$' {
                                self.i += 1;
                                self.varint()?;
                                self.varint()?;
                                break;
                            }
                            self.key()?;
                            self.value()?;
                        }
                        self.ids[id] = Value::Array(arr.clone());
                        Value::Array(arr)
                    }
                    b'\'' => {
                        // Set: values until ',' then a count.
                        let id = self.reserve();
                        let mut arr = Vec::new();
                        while self.peek()? != b',' {
                            arr.push(self.value()?);
                        }
                        self.i += 1;
                        self.varint()?;
                        self.ids[id] = Value::Array(arr.clone());
                        Value::Array(arr)
                    }
                    b';' => {
                        // Map: key/value pairs until ':' then a count.
                        let id = self.reserve();
                        let mut m = Map::new();
                        while self.peek()? != b':' {
                            let k = self.key()?;
                            let v = self.value()?;
                            m.insert(k, v);
                        }
                        self.i += 1;
                        self.varint()?;
                        self.ids[id] = Value::Object(m.clone());
                        Value::Object(m)
                    }
                    b'^' => {
                        let id = self.varint()? as usize;
                        self.ids.get(id).cloned().unwrap_or(Value::Null)
                    }
                    other => {
                        UNKNOWN.with(|u| u.set(Some((self.i - 1, other))));
                        return None;
                    }
                });
            }
        }
    }

    /// Decode an IndexedDB value: skip Blink's envelope and read the V8 payload.
    pub fn decode(b: &[u8]) -> Option<Value> {
        let start = (0..b.len().saturating_sub(2)).find(|&i| b[i] == 0xff && (13..=20).contains(&b[i + 1]) && b[i + 2..].iter().find(|&&c| c != 0) == Some(&b'o'))?;
        let mut r = Reader { b, i: start + 2, ids: Vec::new() };
        r.value()
    }
}

// ---------------------------------------------------------------------------------------------
// Outplayed

struct OpMedia {
    path: String,
    kind: String,
    start_ms: f64,
    events: Vec<RecEvent>,
}

struct OpMatch {
    id: String,
    started: Option<DateTime<Local>>,
    champion: Option<String>,
    kda: Option<Kda>,
    victory: Option<bool>,
    game_mode: Option<String>,
    capture_mode: String,
    medias: Vec<OpMedia>,
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string).filter(|s| !s.is_empty())
}

/// Outplayed event -> this app's event kind (None: no equivalent, skipped).
fn map_event(e: &Map<String, Value>) -> Option<RecEvent> {
    let ty = e.get("type")?.as_str()?;
    let time = num(e.get("time")?)? / 1000.0;
    let timing = e.get("timing").and_then(Value::as_object);
    let before = timing.and_then(|t| t.get("past")).and_then(num).map(|ms| ms / 1000.0);
    let after = timing.and_then(|t| t.get("future")).and_then(num).map(|ms| ms / 1000.0);
    let data: Option<Value> = e.get("data").and_then(Value::as_str).and_then(|s| serde_json::from_str(s.trim_start_matches('\u{1}')).ok());
    let label_key = data.as_ref().and_then(|d| d.get("label")).and_then(Value::as_str).unwrap_or("");
    let (kind, label, multi): (&str, String, Option<u8>) = match ty {
        "kill" => match label_key {
            "double_kill" => ("multikill", "Double Kill".into(), Some(2)),
            "triple_kill" => ("multikill", "Triple Kill".into(), Some(3)),
            "quadra_kill" => ("multikill", "Quadra Kill".into(), Some(4)),
            "penta_kill" => ("multikill", "Penta Kill".into(), Some(5)),
            _ => ("kill", "Kill".into(), None),
        },
        "death" => ("death", "Death".into(), None),
        "assist" => ("assist", "Assist".into(), None),
        "ultimate" => ("ultimate", "Ultimate".into(), None),
        "victory" => ("victory", "Victory".into(), None),
        "turret_kill" => ("turret", "Turret destroyed".into(), None),
        "inhibitor_kill" => ("inhibitor", "Inhibitor destroyed".into(), None),
        "dragon_kill" => ("dragon", "Dragon".into(), None),
        "rift_herald" | "herald_kill" => ("herald", "Rift Herald".into(), None),
        "baron_kill" | "baron" => ("baron", "Baron Nashor".into(), None),
        "manual" | "bookmark" => ("bookmark", "Bookmark".into(), None),
        _ => return None,
    };
    let d = crate::settings::default_timing(kind);
    Some(RecEvent { t: time, kind: kind.into(), label, multi, before: before.unwrap_or(d.before), after: after.unwrap_or(d.after), ..Default::default() })
}

fn ms_to_local(ms: f64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms as i64).single()
}

fn outplayed_db_dir() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?.join("Overwolf").join("CefBrowserCache").join("Default").join("IndexedDB");
    let dir = base.join(format!("overwolf-extension_{OUTPLAYED_EXTENSION}_0.indexeddb.leveldb"));
    dir.exists().then_some(dir)
}

/// Read Outplayed's library database (from a copy, so a running Outplayed is not disturbed).
fn read_outplayed() -> Vec<OpMatch> {
    let Some(src) = outplayed_db_dir() else { return vec![] };
    let tmp = std::env::temp_dir().join(format!("mgr-outplayed-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    if let Ok(rd) = std::fs::read_dir(&src) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "ldb" || x == "log") {
                let _ = std::fs::copy(&p, tmp.join(e.file_name()));
            }
        }
    }
    let mut by_id: HashMap<String, OpMatch> = HashMap::new();
    for (_, v) in leveldb::dump(&tmp) {
        if !v.windows(7).any(|w| w == b"\x06medias") {
            continue;
        }
        let Some(Value::Object(m)) = v8::decode(&v) else { continue };
        let Some(medias) = m.get("medias").and_then(Value::as_array) else { continue };
        let info = m.get("info").and_then(Value::as_object);
        let g = |k: &str| info.and_then(|i| i.get(k));
        let kills = g("killCount").and_then(Value::as_u64);
        let mut op = OpMatch {
            id: m.get("id").and_then(opt_str).unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()),
            started: m.get("startTime").or_else(|| g("startTime")).and_then(num).and_then(ms_to_local),
            champion: g("selectedChampion").and_then(opt_str),
            kda: kills.map(|k| Kda {
                kills: k as u32,
                deaths: g("deathCount").and_then(Value::as_u64).unwrap_or(0) as u32,
                assists: g("assistCount").and_then(Value::as_u64).unwrap_or(0) as u32,
            }),
            victory: None,
            game_mode: g("gameMode").and_then(Value::as_str).map(|mode| if mode == "arena" { "CHERRY".to_string() } else { "CLASSIC".to_string() }),
            capture_mode: m.get("captureMode").or_else(|| g("captureMode")).and_then(Value::as_str).unwrap_or("fullMatch").to_string(),
            medias: medias
                .iter()
                .filter_map(Value::as_object)
                .filter_map(|md| {
                    let start_ms = md.get("startTime").and_then(num).unwrap_or(0.0);
                    let mut events: Vec<RecEvent> = md
                        .get("events")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_object).filter_map(map_event).collect())
                        .unwrap_or_default();
                    // Event times count from the match start; make them relative to this video.
                    if start_ms > 0.0 {
                        for e in &mut events {
                            if e.t * 1000.0 >= start_ms - 1000.0 {
                                e.t -= start_ms / 1000.0;
                            }
                        }
                    }
                    events.retain(|e| e.t >= 0.0);
                    events.sort_by(|a, b| a.t.total_cmp(&b.t));
                    Some(OpMedia { path: md.get("path").and_then(opt_str)?, kind: md.get("type").and_then(Value::as_str).unwrap_or("").to_string(), start_ms, events })
                })
                .collect(),
        };
        op.victory = match_result(g("isVictory").and_then(Value::as_bool), &op.medias);
        by_id.insert(op.id.clone(), op);
    }
    let _ = std::fs::remove_dir_all(&tmp);
    by_id.into_values().collect()
}

/// Outplayed's own `isVictory` is false for most wins (271 of 289 matches on Matt's PC, most of
/// them won), so the result comes from the recordings: a "victory" event means a win, a full
/// recording without one a loss, and highlights without a victory clip leave it unknown.
fn match_result(is_victory: Option<bool>, medias: &[OpMedia]) -> Option<bool> {
    if is_victory == Some(true) || medias.iter().flat_map(|m| &m.events).any(|e| e.kind == "victory") {
        Some(true)
    } else if medias.iter().any(|m| m.kind == "fullMatch" || m.kind == "fullSession") {
        Some(false)
    } else {
        None
    }
}

/// Data Dragon key for a champion display name ("Kai'Sa" -> "Kaisa", "Wukong" -> "MonkeyKing").
fn champion_key(name: &str) -> String {
    let compact: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    match compact.to_lowercase().as_str() {
        "wukong" => "MonkeyKing".into(),
        "nunuwillump" | "nunu" => "Nunu".into(),
        "renataglasc" => "Renata".into(),
        "kaisa" => "Kaisa".into(),
        "khazix" => "Khazix".into(),
        "chogath" => "Chogath".into(),
        "velkoz" => "Velkoz".into(),
        "belveth" => "Belveth".into(),
        "leblanc" => "Leblanc".into(),
        "reksai" => "RekSai".into(),
        "ksante" => "KSante".into(),
        "kogmaw" => "KogMaw".into(),
        _ => compact,
    }
}

fn walk_mp4(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if depth < 4 && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("mp4")) {
                out.push(p);
            }
        }
    }
    out
}

/// "League of Legends_10-30-2026_22-41-37-116" -> 2026-10-30 22:41:37.
fn time_from_name(name: &str) -> Option<DateTime<Local>> {
    let mut parts = name.rsplitn(3, '_');
    let time = parts.next()?;
    let date = parts.next()?;
    let t: Vec<&str> = time.split('-').collect();
    let stamp = format!("{date} {}:{}:{}", t.first()?, t.get(1)?, t.get(2)?);
    let naive = NaiveDateTime::parse_from_str(&stamp, "%m-%d-%Y %H:%M:%S").ok()?;
    Local.from_local_datetime(&naive).single()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutplayedScan {
    /// The Outplayed media folder, when found.
    pub folder: Option<String>,
    pub files: usize,
    pub bytes: u64,
    /// Videos that Outplayed's database has events for.
    pub with_events: usize,
    pub already_imported: usize,
    /// Imported earlier but deleted from the library since (skipped unless re-import is chosen).
    pub deleted_after_import: usize,
    /// Free space on the media folder's drive.
    pub free_bytes: u64,
    /// The Outplayed folder is on the media folder's drive, so deleting originals frees room there.
    pub same_drive: bool,
}

fn guess_outplayed_folder(matches: &[OpMatch]) -> Option<PathBuf> {
    // A media path looks like <root>\<game>\<session folder>\<file>.mp4
    let p = matches.iter().flat_map(|m| &m.medias).map(|md| PathBuf::from(&md.path)).find(|p| p.exists())?;
    let root = p.parent()?.parent()?.parent()?.to_path_buf();
    root.exists().then_some(root)
}

pub fn scan_outplayed(core: &Core, folder: Option<&str>) -> OutplayedScan {
    let matches = read_outplayed();
    // Debug builds: a stand-in Outplayed folder for testing.
    #[cfg(debug_assertions)]
    let folder = folder.map(str::to_string).or_else(|| std::env::var("MGR_FAKE_OUTPLAYED").ok());
    #[cfg(debug_assertions)]
    let folder = folder.as_deref();
    let folder = folder.map(PathBuf::from).or_else(|| guess_outplayed_folder(&matches)).or_else(|| {
        let v = dirs::video_dir()?.join("Overwolf").join("Outplayed");
        v.exists().then_some(v)
    });
    let root = core.library.root();
    let free_bytes = crate::win::disk_space(&root).map_or(0, |(free, _)| free);
    // Debug builds: pretend the drive is this full (GB free), to test the low-space flow.
    #[cfg(debug_assertions)]
    let free_bytes = std::env::var("MGR_FAKE_FREE_GB").ok().and_then(|v| v.parse::<u64>().ok()).map_or(free_bytes, |gb| gb << 30);
    let Some(folder) = folder else {
        return OutplayedScan { folder: None, files: 0, bytes: 0, with_events: 0, already_imported: 0, deleted_after_import: 0, free_bytes, same_drive: false };
    };
    let files = walk_mp4(&folder);
    let known: HashSet<String> = matches.iter().flat_map(|m| &m.medias).filter(|md| !md.events.is_empty()).map(|md| md.path.to_lowercase()).collect();
    let done = core.library.imported_sources();
    let (mut already, mut deleted) = (0, 0);
    for f in &files {
        if done.contains(&f.to_string_lossy().to_lowercase()) {
            already += 1;
        } else if let Some(entry) = crate::ledger::cached_fingerprint(f).and_then(|fp| crate::ledger::source(&fp)) {
            if core.library.has_recording(&entry.recording_id) {
                already += 1;
            } else {
                deleted += 1;
            }
        }
    }
    OutplayedScan {
        folder: Some(folder.to_string_lossy().into_owned()),
        files: files.len(),
        bytes: files.iter().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum(),
        with_events: files.iter().filter(|f| known.contains(&f.to_string_lossy().to_lowercase())).count(),
        already_imported: already,
        deleted_after_import: deleted,
        free_bytes,
        same_drive: crate::win::same_volume(&folder, &root),
    }
}

/// Wait while a game is being recorded: an import must never cost frames in game.
fn wait_while_recording(core: &Core, ctx: &JobCtx, msg: &str, progress: f64) -> Result<()> {
    let mut told = false;
    while core.status.read().state == "recording" {
        if ctx.cancelled() {
            bail!("cancelled");
        }
        if !told {
            ctx.update(progress, &tr!("Paused while you play. {msg}", "游戏中已暂停。{msg}"));
            told = true;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Worker {
    Gpu = 0,
    Cpu = 1,
}

struct Queue {
    /// (position in the plan, video), oldest first.
    items: VecDeque<(usize, Planned)>,
    /// The CPU holds a video; the GPU must not finish while it could still be handed back.
    cpu_busy: bool,
    /// Videos the CPU encoder failed on; only the GPU tries those again.
    cpu_failed: HashSet<usize>,
}

#[derive(Default)]
struct RunState {
    sessions: HashMap<String, Session>,
    /// Output names taken by a conversion in progress (the file only appears when it is done).
    reserved: HashSet<PathBuf>,
    done_bytes: u64,
    /// Bytes' worth of progress inside each worker's current video.
    partial: [f64; 2],
    in_flight: [Option<usize>; 2],
    imported: usize,
    failed: usize,
}

/// One Outplayed import run, shared by the GPU and CPU workers.
struct Run<'a> {
    core: &'a Arc<Core>,
    ctx: &'a JobCtx,
    by_id: HashMap<&'a str, &'a OpMatch>,
    total: u64,
    skipped: usize,
    count: usize,
    bitrate_kbps: u32,
    queue: Mutex<Queue>,
    state: Mutex<RunState>,
    /// A worker hit a fatal error; the other stops too.
    halt: AtomicBool,
    /// Delete each Outplayed original once its conversion is saved and checked (the user asked
    /// for this to free space on a full drive).
    delete_sources: bool,
}

impl Run<'_> {
    fn halted(&self) -> bool {
        self.halt.load(Ordering::Relaxed) || self.ctx.cancelled()
    }

    fn recording(&self) -> bool {
        self.core.status.read().state == "recording"
    }

    /// "Video 36 of 312", or "Videos 36 and 37 of 312, one on the CPU" while both work.
    fn label(&self, s: &RunState) -> String {
        let of = self.skipped + self.count;
        let mut nums: Vec<usize> = s.in_flight.iter().flatten().map(|i| self.skipped + i + 1).collect();
        nums.sort_unstable();
        match nums.as_slice() {
            [a, b] => tr!("Videos {a} and {b} of {of}, one on the CPU", "第 {a} 和第 {b} 个视频，共 {of} 个（其中一个用 CPU）"),
            [a] => tr!("Video {a} of {of}", "第 {a} 个视频，共 {of} 个"),
            _ => {
                let a = (self.skipped + s.imported + s.failed + 1).min(of);
                tr!("Video {a} of {of}", "第 {a} 个视频，共 {of} 个")
            }
        }
    }

    fn report(&self) {
        let (p, label) = {
            let s = self.state.lock();
            ((s.done_bytes as f64 + s.partial[0] + s.partial[1]) / self.total as f64, self.label(&s))
        };
        self.ctx.update(p, &label);
    }

    fn gpu_worker(&self, encoder: &str) -> Result<()> {
        loop {
            let next = {
                let mut q = self.queue.lock();
                match q.items.pop_front() {
                    Some(item) => Some(item),
                    None if q.cpu_busy => None,
                    None => return Ok(()),
                }
            };
            if self.halted() {
                return if self.ctx.cancelled() { Err(anyhow::anyhow!("cancelled")) } else { Ok(()) };
            }
            match next {
                Some((i, item)) => {
                    self.convert_one(Worker::Gpu, encoder, i, item)?;
                }
                // Waiting in case the CPU hands its video back.
                None => std::thread::sleep(Duration::from_millis(500)),
            }
        }
    }

    fn cpu_worker(&self) -> Result<()> {
        loop {
            if self.halted() {
                return Ok(());
            }
            if !cpu_assist() || self.ctx.is_paused() || self.recording() {
                if self.queue.lock().items.is_empty() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            let next = {
                let mut guard = self.queue.lock();
                let q = &mut *guard;
                let n = q.items.iter().position(|(i, _)| !q.cpu_failed.contains(i)).and_then(|at| q.items.remove(at));
                q.cpu_busy = n.is_some();
                n
            };
            let Some((i, item)) = next else { return Ok(()) };
            let r = self.convert_one(Worker::Cpu, CPU_ENCODER, i, item);
            let mut q = self.queue.lock();
            q.cpu_busy = false;
            match r {
                Ok(None) => {}
                Ok(Some(back)) => q.items.push_front(back),
                Err(e) => {
                    self.halt.store(true, Ordering::Relaxed);
                    return Err(e);
                }
            }
        }
    }

    /// Convert one video. Returns it back when the CPU had to let go of it (help turned off,
    /// or a game started), so the GPU can take it.
    fn convert_one(&self, who: Worker, encoder: &str, i: usize, item: Planned) -> Result<Option<(usize, Planned)>> {
        let core = self.core;
        let ctx = self.ctx;
        ctx.wait_if_paused()?;
        if who == Worker::Gpu {
            let (p, label) = {
                let s = self.state.lock();
                (s.done_bytes as f64 / self.total as f64, self.label(&s))
            };
            wait_while_recording(core, ctx, &label, p)?;
        }
        if ctx.cancelled() {
            bail!("cancelled");
        }
        if let Some((free, _)) = crate::win::disk_space(&core.library.root()) {
            if free < item.size + 2 * 1024 * 1024 * 1024 {
                let n = self.state.lock().imported;
                bail!(tr!("Not enough free space on the media drive to continue ({n} imported so far).", "媒体所在磁盘空间不足，无法继续（已导入 {n} 个）。"));
            }
        }

        let (session, title, out) = {
            let mut s = self.state.lock();
            let session = self.session_for(&mut s, &item)?;
            let title = match item.kind.as_str() {
                "match" | "session" => session.title.clone(),
                _ => item.events.iter().max_by_key(|e| e.multi.unwrap_or(0) as u32 * 10 + (e.kind == "kill") as u32).map(|e| e.label.clone()).unwrap_or_else(|| if item.kind == "highlight" { "Replay".into() } else { "Clip".into() }),
            };
            let out = reserve_file(&session.dir, &title, &mut s.reserved);
            s.in_flight[who as usize] = Some(i);
            (session, title, out)
        };
        self.report();
        // Written under a temporary name and renamed when complete, so an interrupted
        // conversion never looks like a finished video (startup sweeps these up).
        let tmp = out.with_extension(PARTIAL_EXT);
        let size = item.size.max(1);
        let release = |s: &mut RunState| {
            s.reserved.remove(&out);
            s.in_flight[who as usize] = None;
            s.partial[who as usize] = 0.0;
        };
        let result = loop {
            let r = crate::ffmpeg::convert(
                &item.src,
                &tmp,
                encoder,
                self.bitrate_kbps,
                &ctx.cancel,
                &ctx.paused,
                || self.halted() || self.recording() || (who == Worker::Cpu && !cpu_assist()),
                |p| {
                    self.state.lock().partial[who as usize] = p * size as f64;
                    self.report();
                },
            );
            if matches!(r, Ok(false)) {
                // Stopped early; the partial file is gone.
                if who == Worker::Cpu || self.halted() {
                    release(&mut self.state.lock());
                    self.report();
                    return Ok(if self.halted() { None } else { Some((i, item)) });
                }
                // A match started: wait, then redo this video.
                self.state.lock().partial[who as usize] = 0.0;
                let (p, label) = {
                    let s = self.state.lock();
                    (s.done_bytes as f64 / self.total as f64, self.label(&s))
                };
                wait_while_recording(core, ctx, &label, p)?;
                continue;
            }
            break r;
        };
        match result {
            Ok(true) => {
                let mut s = self.state.lock();
                let finalize = || -> Result<f64> {
                    std::fs::rename(&tmp, &out).context(tr!("could not finalize the converted video", "无法完成转换后的视频"))?;
                    let mut r = core.library.add_file(&session, &item.kind, &title, &out, item.events.clone(), vec![], None)?;
                    r.imported_from = Some(item.src.to_string_lossy().into_owned());
                    r.source_fingerprint = Some(item.fp.clone());
                    core.library.save_recording(&mut r)?;
                    crate::ledger::record(&item.fp, &item.src, &r.id);
                    crate::ledger::save();
                    let _ = core.library.renumber_matches(&session.game, session.started_at.date_naive());
                    Ok(r.duration)
                };
                let done = finalize();
                release(&mut s);
                s.done_bytes += item.size;
                let duration = done?;
                s.imported += 1;
                drop(s);
                if self.delete_sources {
                    delete_original(&item.src, duration);
                }
                if let Some(key) = session.champion_key.clone() {
                    crate::lol::art::ensure(&key);
                }
                core.library_changed();
                // The speed estimate follows real conversions, so keep the toggle's number current.
                crate::commands::emit_cpu_assist(core);
            }
            Ok(false) => unreachable!("handled above"),
            Err(e) if ctx.cancelled() => {
                let _ = std::fs::remove_file(&tmp);
                release(&mut self.state.lock());
                return Err(e);
            }
            // The GPU gets another go at anything the CPU encoder could not handle.
            Err(e) if who == Worker::Cpu => {
                log::warn!("CPU could not convert {}, leaving it to the GPU: {e:#}", item.src.display());
                let _ = std::fs::remove_file(&tmp);
                release(&mut self.state.lock());
                self.queue.lock().cpu_failed.insert(i);
                self.report();
                return Ok(Some((i, item)));
            }
            Err(e) => {
                log::error!("could not convert {}: {e:#}", item.src.display());
                let _ = std::fs::remove_file(&tmp);
                let mut s = self.state.lock();
                release(&mut s);
                s.failed += 1;
                s.done_bytes += item.size;
            }
        }
        self.report();
        Ok(None)
    }

    /// The session (match) a video belongs to, created on first use.
    fn session_for(&self, s: &mut RunState, item: &Planned) -> Result<Session> {
        if let Some(found) = s.sessions.get(&item.match_id) {
            return Ok(found.clone());
        }
        let core = self.core;
        let game = if item.game.eq_ignore_ascii_case("screen recorder") { "Screen recorder".to_string() } else { item.game.clone() };
        let op = self.by_id.get(item.match_id.as_str());
        let n = core.library.next_match_number_on(&game, item.when.date_naive());
        let noun = if game == "Screen recorder" { "Screen recording" } else if item.kind == "session" { "Session" } else { "Match" };
        let mut session = core.library.create_session_at(&game, &format!("{noun} {n:02}"), "match", item.when)?;
        if let Some(op) = op {
            session.champion = op.champion.clone();
            session.champion_key = op.champion.as_deref().map(champion_key);
            session.kda = op.kda.clone();
            session.result = op.victory.map(|v| if v { "win".into() } else { "loss".into() });
            session.game_mode = op.game_mode.clone();
            session.capture_mode = if op.capture_mode == "manual" { "manual".into() } else { "match".into() };
        }
        core.library.save_session(&session)?;
        s.sessions.insert(item.match_id.clone(), session.clone());
        Ok(session)
    }
}

/// `paths::unique_file`, also avoiding names another conversion in progress has claimed.
fn reserve_file(dir: &Path, stem: &str, reserved: &mut HashSet<PathBuf>) -> PathBuf {
    let stem = paths::sanitize(stem);
    let out = std::iter::once(dir.join(format!("{stem}.mp4")))
        .chain((2..).map(|n| dir.join(format!("{stem} ({n}).mp4"))))
        .find(|p| !p.exists() && !reserved.contains(p))
        .unwrap();
    reserved.insert(out.clone());
    out
}

struct Planned {
    src: PathBuf,
    fp: String,
    size: u64,
    match_id: String,
    kind: String,
    events: Vec<RecEvent>,
    when: DateTime<Local>,
    game: String,
}

// ---------------------------------------------------------------------------------------------
// Resume after restart: a running import is remembered until it finishes or is cancelled.

fn pending_path() -> PathBuf {
    paths::config_dir().join("pending-import.json")
}

fn remember(kind: &str, folder: &Path, paused: bool, opts: Options) {
    let v = serde_json::json!({ "kind": kind, "folder": folder, "paused": paused, "includeDeleted": opts.include_deleted, "deleteSources": opts.delete_sources });
    let _ = std::fs::write(pending_path(), v.to_string());
}

/// Choices for an Outplayed import.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Re-import videos whose earlier import was deleted from the library.
    pub include_deleted: bool,
    /// Delete each original once it is imported (asked for when the drive is too full).
    pub delete_sources: bool,
}

/// After a successful conversion: delete the Outplayed original, but only when the converted
/// video is as long as it (a short or unreadable copy keeps the original).
fn delete_original(src: &Path, converted: f64) {
    let original = crate::ffmpeg::probe(src).map(|p| p.duration).unwrap_or(0.0);
    if original <= 0.0 || converted + (original * 0.02).max(2.0) < original {
        log::warn!("kept {}: converted copy is {converted:.1}s of {original:.1}s", src.display());
        return;
    }
    match std::fs::remove_file(src) {
        Ok(()) => log::info!("deleted the imported original {}", src.display()),
        Err(e) => log::warn!("could not delete the imported original {}: {e}", src.display()),
    }
}

/// Keep the pause state of the pending import, so a paused import stays paused after a restart.
pub fn set_pending_paused(paused: bool) {
    if let Some(p) = pending() {
        remember(&p.kind, &p.folder, paused, p.options);
    }
}

/// Start an import job. `kind` is "outplayed" or "library". `include_deleted` re-imports
/// Outplayed videos whose earlier import was deleted from the library.
pub fn start(core: &Arc<Core>, kind: &str, folder: PathBuf, paused: bool, opts: Options) -> String {
    let c = core.clone();
    let (tag, title, outplayed) = if kind == "library" {
        ("library", tr!("Importing recordings", "正在导入录像"), false)
    } else {
        ("outplayed", tr!("Importing from Outplayed", "正在从 Outplayed 导入"), true)
    };
    core.jobs.spawn_with(&core.app, "import", tag, &title, true, paused, move |ctx| {
        if outplayed {
            import_outplayed(&c, &folder, ctx, opts)
        } else {
            import_library(&c, &folder, ctx)
        }
    })
}

/// Called when an import ends. A process that dies mid-import never gets here, so the
/// import resumes at the next launch.
fn forget() {
    let _ = std::fs::remove_file(pending_path());
}

pub struct Pending {
    pub kind: String,
    pub folder: PathBuf,
    pub paused: bool,
    pub options: Options,
}

/// An import interrupted by a restart or crash.
pub fn pending() -> Option<Pending> {
    let v: Value = serde_json::from_slice(&std::fs::read(pending_path()).ok()?).ok()?;
    Some(Pending {
        kind: v.get("kind")?.as_str()?.to_string(),
        folder: PathBuf::from(v.get("folder")?.as_str()?),
        paused: v.get("paused").and_then(Value::as_bool).unwrap_or(false),
        options: Options {
            include_deleted: v.get("includeDeleted").and_then(Value::as_bool).unwrap_or(false),
            delete_sources: v.get("deleteSources").and_then(Value::as_bool).unwrap_or(false),
        },
    })
}

pub fn import_outplayed(core: &Arc<Core>, folder: &Path, ctx: &JobCtx, opts: Options) -> Result<Option<String>> {
    remember("outplayed", folder, ctx.is_paused(), opts);
    let result = import_outplayed_inner(core, folder, ctx, opts.include_deleted, opts.delete_sources);
    forget();
    // Drop sessions created for videos that never finished (cancel, error, full disk).
    if core.library.recover_partials() > 0 || result.is_err() {
        core.library_changed();
    }
    result
}

/// " Skipped 3 duplicates and 2 you deleted earlier." (empty when nothing was skipped for those reasons)
fn skip_note(duplicates: usize, deleted: usize) -> String {
    if crate::i18n::zh() {
        return match (duplicates, deleted) {
            (0, 0) => String::new(),
            (d, 0) => format!("已跳过 {d} 个重复视频。"),
            (0, x) => format!("已跳过 {x} 个你之前删除的视频。"),
            (d, x) => format!("已跳过 {d} 个重复视频和 {x} 个你之前删除的视频。"),
        };
    }
    let plural = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    match (duplicates, deleted) {
        (0, 0) => String::new(),
        (d, 0) => format!(" Skipped {}.", plural(d, "duplicate", "duplicates")),
        (0, x) => format!(" Skipped {} you deleted earlier.", plural(x, "video", "videos")),
        (d, x) => format!(" Skipped {} and {} you deleted earlier.", plural(d, "duplicate", "duplicates"), plural(x, "video", "videos")),
    }
}

/// Fingerprint the sources of recordings imported before the ledger existed.
fn backfill_ledger(core: &Core, ctx: &JobCtx) {
    let old = core.library.imported_without_fingerprint();
    if old.is_empty() {
        return;
    }
    let n = old.len();
    for (i, mut r) in old.into_iter().enumerate() {
        let i = i + 1;
        ctx.update(0.0, &tr!("Updating the import history ({i} of {n})", "正在更新导入记录（{i}/{n}）"));
        let Some(src) = r.imported_from.clone().map(PathBuf::from) else { continue };
        if let Ok(fp) = crate::ledger::fingerprint(&src) {
            crate::ledger::record_if_missing(&fp, &src, &r.id);
            r.source_fingerprint = Some(fp);
            let _ = core.library.save_recording(&mut r);
        }
    }
    crate::ledger::save();
}

fn import_outplayed_inner(core: &Arc<Core>, folder: &Path, ctx: &JobCtx, include_deleted: bool, delete_sources: bool) -> Result<Option<String>> {
    if !folder.exists() {
        let f = folder.display();
        bail!(tr!("Folder not found: {f}", "找不到文件夹：{f}"));
    }
    ctx.update(0.0, &tr!("Reading the Outplayed library...", "正在读取 Outplayed 资料库..."));
    let matches = read_outplayed();
    let mut media_of: HashMap<String, (&OpMatch, &OpMedia)> = HashMap::new();
    for m in &matches {
        for md in &m.medias {
            media_of.insert(md.path.to_lowercase(), (m, md));
        }
    }
    backfill_ledger(core, ctx);
    let done = core.library.imported_sources();
    let mut plan: Vec<Planned> = Vec::new();
    // Already-imported videos count toward progress, so a resumed import shows overall progress.
    let (mut skipped, mut skipped_bytes) = (0usize, 0u64);
    let (mut duplicates, mut deleted_before) = (0usize, 0usize);
    let mut seen: HashSet<String> = HashSet::new();
    let files = walk_mp4(folder);
    let n_files = files.len();
    for (i, src) in files.into_iter().enumerate() {
        if ctx.cancelled() {
            bail!("cancelled");
        }
        if i % 10 == 0 {
            let i = i + 1;
            ctx.update(0.0, &tr!("Checking for duplicates ({i} of {n_files})", "正在检查重复（{i}/{n_files}）"));
        }
        let key = src.to_string_lossy().to_lowercase();
        let size = src.metadata().map(|m| m.len()).unwrap_or(0);
        if done.contains(&key) {
            skipped += 1;
            skipped_bytes += size;
            if let Some(fp) = crate::ledger::cached_fingerprint(&src) {
                seen.insert(fp);
            }
            continue;
        }
        // Content check: catches moved, renamed and copied files, and videos deleted after import.
        let fp = match crate::ledger::fingerprint(&src) {
            Ok(fp) => fp,
            Err(e) => {
                log::warn!("cannot read {}: {e}", src.display());
                continue;
            }
        };
        if !seen.insert(fp.clone()) {
            duplicates += 1;
            skipped += 1;
            skipped_bytes += size;
            continue;
        }
        if let Some(entry) = crate::ledger::source(&fp) {
            let still_here = core.library.has_recording(&entry.recording_id);
            if still_here || !include_deleted {
                if still_here {
                    duplicates += 1;
                } else {
                    deleted_before += 1;
                }
                skipped += 1;
                skipped_bytes += size;
                continue;
            }
        }
        let rel = src.strip_prefix(folder).unwrap_or(&src);
        let game = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_else(|| "League of Legends".into());
        let session_dir = src.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = src.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (match_id, kind, events, when) = match media_of.get(&key) {
            Some((m, md)) => {
                let kind = match md.kind.as_str() {
                    "fullMatch" => "match",
                    "fullSession" => "session",
                    "replay" => "highlight",
                    _ => "clip",
                };
                let when = m.started.map(|s| s + chrono::Duration::milliseconds(md.start_ms as i64)).or_else(|| time_from_name(&stem)).unwrap_or_else(Local::now);
                (m.id.clone(), kind.to_string(), md.events.clone(), when)
            }
            None => {
                let when = time_from_name(&stem).or_else(|| time_from_name(&session_dir)).unwrap_or_else(Local::now);
                (format!("folder:{session_dir}"), "match".to_string(), vec![], when)
            }
        };
        plan.push(Planned { src, fp, size, match_id, kind, events, when, game });
    }
    crate::ledger::save();
    let skipped_note = skip_note(duplicates, deleted_before);
    if plan.is_empty() {
        ctx.update(1.0, &tr!("Everything is already imported.{skipped_note}", "所有内容都已导入。{skipped_note}"));
        return Ok(None);
    }
    // Oldest first, so match numbers per day come out in play order.
    plan.sort_by_key(|p| p.when);
    let total: u64 = (plan.iter().map(|p| p.size).sum::<u64>() + skipped_bytes).max(1);
    let settings = core.settings();
    let encoder = crate::ffmpeg::best_encoder(&settings.video.encoder);
    let run = Run {
        core,
        ctx,
        by_id: matches.iter().map(|m| (m.id.as_str(), m)).collect(),
        total,
        skipped,
        count: plan.len(),
        bitrate_kbps: settings.video.bitrate_kbps,
        queue: Mutex::new(Queue { items: plan.into_iter().enumerate().collect(), cpu_busy: false, cpu_failed: HashSet::new() }),
        state: Mutex::new(RunState { done_bytes: skipped_bytes, ..Default::default() }),
        halt: AtomicBool::new(false),
        delete_sources,
    };
    // The GPU converts the whole time. With CPU help on, the processor converts another video
    // alongside it; turning help off hands that video back to the GPU.
    let helper = crate::ffmpeg::family(&encoder) != "cpu";
    std::thread::scope(|scope| -> Result<()> {
        let cpu = helper.then(|| scope.spawn(|| run.cpu_worker()));
        let gpu = run.gpu_worker(&encoder);
        if gpu.is_err() {
            run.halt.store(true, Ordering::Relaxed);
        }
        let cpu = cpu.map_or(Ok(()), |h| h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("the CPU encoder crashed"))));
        gpu.and(cpu)
    })?;
    let (imported, failed) = {
        let s = run.state.lock();
        (s.imported, s.failed)
    };
    let videos = if imported == 1 { "video" } else { "videos" };
    ctx.update(
        1.0,
        &if failed > 0 {
            tr!("Imported {imported} {videos}; {failed} could not be converted (see app.log).{skipped_note}", "已导入 {imported} 个视频；{failed} 个无法转换（见 app.log）。{skipped_note}")
        } else {
            tr!("Imported {imported} {videos}.{skipped_note}", "已导入 {imported} 个视频。{skipped_note}")
        },
    );
    Ok(None)
}

// ---------------------------------------------------------------------------------------------
// Another Matt Game Recorder library

pub fn import_library(core: &Arc<Core>, folder: &Path, ctx: &JobCtx) -> Result<Option<String>> {
    remember("library", folder, ctx.is_paused(), Options::default());
    let result = import_library_inner(core, folder, ctx);
    forget();
    result
}

/// A session folder still holds a recording (a sidecar other than the session's own).
fn has_recordings(dir: &Path) -> bool {
    std::fs::read_dir(dir).map_or(true, |rd| {
        rd.flatten().any(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.ends_with(".mgr.json") && n != SESSION_FILE
        })
    })
}

/// Fingerprints of every video already in the library (cached after the first run).
fn our_media_fingerprints(core: &Core, ctx: &JobCtx) -> HashSet<String> {
    let recs = core.library.snapshot().recordings;
    let n = recs.len();
    let set = recs
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            if i % 10 == 0 {
                let i = i + 1;
                ctx.update(0.0, &tr!("Checking your library for duplicates ({i} of {n})", "正在检查资料库中的重复（{i}/{n}）"));
            }
            crate::ledger::fingerprint(&r.path()).ok()
        })
        .collect();
    crate::ledger::save();
    set
}

fn import_library_inner(core: &Arc<Core>, folder: &Path, ctx: &JobCtx) -> Result<Option<String>> {
    let root = core.library.root();
    if !folder.exists() {
        let f = folder.display();
        bail!(tr!("Folder not found: {f}", "找不到文件夹：{f}"));
    }
    if folder.canonicalize().ok() == root.canonicalize().ok() {
        bail!(tr!("That is this app's own media folder.", "这是本应用自己的媒体文件夹。"));
    }
    ctx.update(0.0, &tr!("Scanning...", "正在扫描..."));
    // Find session folders.
    let mut session_dirs = Vec::new();
    let mut stack = vec![(folder.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if dir.join(SESSION_FILE).exists() {
            session_dirs.push(dir.clone());
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if e.path().is_dir() && depth < 3 && !SKIP_DIRS.contains(&name.as_str()) {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    if session_dirs.is_empty() {
        let f = folder.display();
        bail!(tr!("No Matt Game Recorder recordings found in {f}.", "在 {f} 中没有找到 Matt Game Recorder 录像。"));
    }

    struct Copy {
        files: Vec<String>,
        sidecar: crate::library::Recording,
        dir: PathBuf,
        media_fp: String,
    }
    let mut work: Vec<(Session, PathBuf, Vec<Copy>)> = Vec::new();
    let mut total = 0u64;
    let our_sources = core.library.imported_sources();
    let mut ours_media: Option<HashSet<String>> = None;
    let mut seen: HashSet<String> = HashSet::new();
    let mut duplicates = 0usize;
    for dir in session_dirs {
        let Ok(session) = serde_json::from_slice::<Session>(&std::fs::read(dir.join(SESSION_FILE))?) else { continue };
        let mut copies = Vec::new();
        for e in std::fs::read_dir(&dir)?.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == SESSION_FILE || !name.ends_with(".mgr.json") {
                continue;
            }
            let Ok(rec) = serde_json::from_slice::<crate::library::Recording>(&std::fs::read(e.path())?) else { continue };
            if rec.deleted_at.is_some() || !dir.join(&rec.file).exists() {
                continue;
            }
            // Same recording, same original source, or the same video content under any name.
            let same_source = rec.source_fingerprint.as_deref().is_some_and(|fp| crate::ledger::source(fp).is_some())
                || rec.imported_from.as_ref().is_some_and(|p| our_sources.contains(&p.to_lowercase()));
            if core.library.has_recording(&rec.id) || same_source {
                duplicates += 1;
                continue;
            }
            let shown = crate::i18n::title(&rec.title);
            ctx.update(0.0, &tr!("Checking for duplicates: {shown}", "正在检查重复：{shown}"));
            let Ok(media_fp) = crate::ledger::fingerprint(&dir.join(&rec.file)) else { continue };
            let ours = ours_media.get_or_insert_with(|| our_media_fingerprints(core, ctx));
            if !seen.insert(media_fp.clone()) || crate::ledger::source(&media_fp).is_some() || ours.contains(&media_fp) {
                duplicates += 1;
                continue;
            }
            let mut files = vec![rec.file.clone()];
            files.extend(rec.thumb.clone());
            total += files.iter().filter_map(|f| dir.join(f).metadata().ok()).map(|m| m.len()).sum::<u64>();
            copies.push(Copy { files, sidecar: rec, dir: dir.clone(), media_fp });
        }
        if !copies.is_empty() {
            work.push((session, dir, copies));
        }
    }
    crate::ledger::save();
    let skipped_note = skip_note(duplicates, 0);
    if work.is_empty() {
        ctx.update(1.0, &tr!("Everything is already imported.{skipped_note}", "所有内容都已导入。{skipped_note}"));
        return Ok(None);
    }
    let total = total.max(1);
    let count: usize = work.iter().map(|(_, _, c)| c.len()).sum();
    let mut done = 0u64;
    let mut imported = 0usize;
    // On the same drive the files move (a rename: instant, no extra space); otherwise they copy.
    let moving = crate::win::same_volume(folder, &root);
    for (session, src_dir, copies) in work {
        if ctx.cancelled() {
            bail!("cancelled");
        }
        let dest = match core.library.session(&session.id) {
            Some(existing) => existing.dir,
            None => {
                let base = root.join(paths::sanitize(&session.game)).join(src_dir.file_name().context("bad folder")?);
                let mut d = base.clone();
                let mut n = 2;
                while d.exists() {
                    d = PathBuf::from(format!("{} ({n})", base.display()));
                    n += 1;
                }
                std::fs::create_dir_all(&d)?;
                let mut s = session.clone();
                s.dir = d.clone();
                core.library.save_session(&s)?;
                d
            }
        };
        for c in copies {
            for f in &c.files {
                let from = c.dir.join(f);
                let size = from.metadata().map(|m| m.len()).unwrap_or(0);
                if !(moving && std::fs::rename(&from, dest.join(f)).is_ok()) {
                    std::fs::copy(&from, dest.join(f)).with_context(|| format!("copying {}", from.display()))?;
                }
                done += size;
                if moving {
                    ctx.update(done as f64 / total as f64, &tr!("{imported} of {count} moved", "已移动 {imported}/{count}"));
                } else {
                    ctx.update(done as f64 / total as f64, &tr!("{imported} of {count} copied", "已复制 {imported}/{count}"));
                }
            }
            // A moved recording leaves the other folder entirely (its sidecar goes too).
            if moving && !c.dir.join(&c.sidecar.file).exists() {
                let _ = std::fs::remove_file(crate::library::sidecar_for(&c.dir, &c.sidecar.file));
            }
            let mut rec = c.sidecar;
            rec.dir = dest.clone();
            let from = c.dir.join(&rec.file);
            rec.imported_from.get_or_insert_with(|| from.to_string_lossy().into_owned());
            core.library.save_recording(&mut rec)?;
            // Remember both the file itself and, for converted Outplayed videos, its original.
            crate::ledger::record(&c.media_fp, &from, &rec.id);
            if let (Some(sfp), Some(orig)) = (&rec.source_fingerprint, &rec.imported_from) {
                crate::ledger::record_if_missing(sfp, Path::new(orig), &rec.id);
            }
            crate::ledger::save();
            imported += 1;
        }
        core.library_changed();
        // A session folder whose recordings all moved goes too.
        if moving && !has_recordings(&src_dir) {
            let _ = std::fs::remove_file(src_dir.join(SESSION_FILE));
            let _ = std::fs::remove_dir(&src_dir);
        }
    }
    core.library.reload();
    let s = if imported == 1 { "" } else { "s" };
    ctx.update(1.0, &tr!("Imported {imported} recording{s}.{skipped_note}", "已导入 {imported} 个录像。{skipped_note}"));
    Ok(None)
}

/// Summary of what Outplayed's database holds (used by the dev example and logs).
pub fn outplayed_summary() -> String {
    let m = read_outplayed();
    let medias: usize = m.iter().map(|x| x.medias.len()).sum();
    let events: usize = m.iter().flat_map(|x| &x.medias).map(|md| md.events.len()).sum();
    let mut kinds: HashMap<String, usize> = HashMap::new();
    for e in m.iter().flat_map(|x| &x.medias).flat_map(|md| &md.events) {
        *kinds.entry(e.kind.clone()).or_default() += 1;
    }
    let sample = m.iter().find(|x| x.medias.iter().any(|md| md.path.contains("22-41-37-116")));
    let sample = sample.map(|x| {
        let md = x.medias.iter().find(|md| md.path.contains("22-41-37-116")).unwrap();
        format!(
            "sample: champ={:?} kda={:?} victory={:?} mode={:?} started={:?} kind={} first events={:?}",
            x.champion,
            x.kda.as_ref().map(|k| (k.kills, k.deaths, k.assists)),
            x.victory,
            x.game_mode,
            x.started,
            md.kind,
            md.events.iter().take(4).map(|e| (e.t, e.kind.clone(), e.before, e.after)).collect::<Vec<_>>()
        )
    });
    format!("matches {} medias {} events {} kinds {:?}\n{}", m.len(), medias, events, kinds, sample.unwrap_or_default())
}

/// Raw match records from an Outplayed database folder (or a copy of one) whose champion
/// matches `champion`, with the per-video data left out. For investigating Outplayed's format.
#[doc(hidden)]
pub fn debug_match_info(dir: &Path, champion: &str) -> String {
    let mut out = String::new();
    for (_, v) in leveldb::dump(dir) {
        if !v.windows(7).any(|w| w == b"medias") {
            continue;
        }
        let Some(Value::Object(mut m)) = v8::decode(&v) else { continue };
        let champ = m.get("info").and_then(|i| i.get("selectedChampion")).and_then(Value::as_str).unwrap_or("");
        if !champ.eq_ignore_ascii_case(champion) {
            continue;
        }
        let medias = m.remove("medias").and_then(|x| x.as_array().map(|a| a.len())).unwrap_or(0);
        out.push_str(&format!("medias={medias} {}
", Value::Object(m)));
    }
    out
}

#[doc(hidden)]
pub fn debug_counts() -> String {
    let Some(src) = outplayed_db_dir() else { return "no db dir".into() };
    let kv = leveldb::dump(&src);
    let with = kv.iter().filter(|(_, v)| v.windows(7).any(|w| w == b"\x06medias")).collect::<Vec<_>>();
    let decoded = with.iter().filter(|(_, v)| v8::decode(v).is_some()).count();
    let first = with
        .iter()
        .find(|(_, v)| v8::decode(v).is_none())
        .map(|(_, v)| {
            let (pos, tag) = v8::UNKNOWN.with(|u| u.get()).unwrap_or((0, 0));
            format!("fails at {pos} tag {:?} ({tag}) context {:?}", tag as char, String::from_utf8_lossy(&v[pos.saturating_sub(60)..(pos + 40).min(v.len())]))
        })
        .unwrap_or_default();
    format!("dir {} kv {} with medias {} decoded {} first {}", src.display(), kv.len(), with.len(), decoded, first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(kind: &str, events: &[&str]) -> OpMedia {
        let events = events.iter().map(|k| RecEvent { t: 1.0, kind: k.to_string(), ..Default::default() }).collect();
        OpMedia { path: String::new(), kind: kind.into(), start_ms: 0.0, events }
    }

    #[test]
    fn result_comes_from_the_recordings() {
        // Outplayed says "not a victory", but the match has a victory event: a win.
        assert_eq!(match_result(Some(false), &[media("fullMatch", &["kill", "victory"])]), Some(true));
        // A victory clip among highlights is a win too.
        assert_eq!(match_result(Some(false), &[media("replay", &["victory"])]), Some(true));
        // A full match without a victory event: a loss.
        assert_eq!(match_result(Some(false), &[media("fullMatch", &["kill", "death"])]), Some(false));
        // Highlights only, no victory clip: unknown.
        assert_eq!(match_result(Some(false), &[media("replay", &["kill"])]), None);
        // Outplayed's flag is trusted when it does say victory.
        assert_eq!(match_result(Some(true), &[media("fullMatch", &[])]), Some(true));
    }
}
