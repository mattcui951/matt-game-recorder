//! Import ledger: remembers every imported source video by a content fingerprint, so imports
//! never bring in the same video twice, even when it was moved, renamed or copied, imported
//! through a different route, or deleted from the library afterwards.
//!
//! Stored in %APPDATA%/Matt Game Recorder/import-ledger.json:
//!   sources:   fingerprint -> where it came from and which recording it became
//!   fileCache: path -> (size, modified, fingerprint), so unchanged files are hashed only once

use chrono::{DateTime, Local};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

/// Bytes hashed from the start and from the end of a file.
const SAMPLE: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEntry {
    /// Path the source had when it was imported.
    pub path: String,
    /// Recording it became (may since have been deleted).
    pub recording_id: String,
    pub imported_at: DateTime<Local>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CachedFile {
    size: u64,
    modified: u64,
    fp: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Data {
    sources: HashMap<String, SourceEntry>,
    file_cache: HashMap<String, CachedFile>,
}

fn path() -> PathBuf {
    crate::paths::config_dir().join("import-ledger.json")
}

fn data() -> &'static Mutex<Data> {
    static D: OnceLock<Mutex<Data>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()))
}

/// Write the ledger to disk (call after a batch of changes).
pub fn save() {
    let d = data().lock();
    let tmp = path().with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(&*d) {
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, path());
        }
    }
}

/// 64-bit FNV-1a: tiny, fast and stable across versions (unlike std's hasher).
fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn compute(file: &Path, size: u64) -> std::io::Result<String> {
    let mut f = File::open(file)?;
    let mut buf = vec![0u8; SAMPLE.min(size) as usize];
    f.read_exact(&mut buf)?;
    // Two independent 64-bit hashes over the head and tail samples.
    let mut a = fnv1a(0xcbf2_9ce4_8422_2325, &buf);
    let mut b = fnv1a(0x6c62_272e_07bb_0142, &buf);
    if size > SAMPLE {
        let tail = SAMPLE.min(size - SAMPLE);
        f.seek(SeekFrom::End(-(tail as i64)))?;
        buf.resize(tail as usize, 0);
        f.read_exact(&mut buf)?;
        a = fnv1a(a, &buf);
        b = fnv1a(b, &buf);
    }
    Ok(format!("{size:x}-{a:016x}{b:016x}"))
}

/// Content fingerprint of a video file: its size plus hashes of its first and last 4 MB.
/// Identical copies match wherever they live and whatever they are called.
pub fn fingerprint(file: &Path) -> std::io::Result<String> {
    let meta = std::fs::metadata(file)?;
    let size = meta.len();
    let modified = meta.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let key = file.to_string_lossy().to_lowercase();
    if let Some(c) = data().lock().file_cache.get(&key) {
        if c.size == size && c.modified == modified {
            return Ok(c.fp.clone());
        }
    }
    let fp = compute(file, size)?;
    data().lock().file_cache.insert(key, CachedFile { size, modified, fp: fp.clone() });
    Ok(fp)
}

/// Fingerprint already known for this path without reading the file (None if never hashed
/// or the file changed since).
pub fn cached_fingerprint(file: &Path) -> Option<String> {
    let meta = std::fs::metadata(file).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let d = data().lock();
    let c = d.file_cache.get(&file.to_string_lossy().to_lowercase())?;
    (c.size == meta.len() && c.modified == modified).then(|| c.fp.clone())
}

pub fn source(fp: &str) -> Option<SourceEntry> {
    data().lock().sources.get(fp).cloned()
}

pub fn record(fp: &str, source_path: &Path, recording_id: &str) {
    data().lock().sources.insert(
        fp.to_string(),
        SourceEntry { path: source_path.to_string_lossy().into_owned(), recording_id: recording_id.to_string(), imported_at: Local::now() },
    );
}

/// Record an entry only if the fingerprint is new (backfilling older imports).
pub fn record_if_missing(fp: &str, source_path: &Path, recording_id: &str) {
    if source(fp).is_none() {
        record(fp, source_path, recording_id);
    }
}
