//! The media library. Everything lives in the media folder as plain files:
//!
//!   <media>/<game>/<session stamp>/session.mgr.json      one per match / recording session
//!   <media>/<game>/<session stamp>/<name>.mp4             a recording
//!   <media>/<game>/<session stamp>/<name>.mgr.json        its metadata (events, APM, ...)
//!   <media>/<game>/<session stamp>/<name>.thumb.jpg       its thumbnail
//!   <media>/Recently Deleted/<id>/...                     soft-deleted recordings
//!   <media>/.cache/                                       regenerable sprites
//!
//! The folder is the source of truth (copy it and you have a backup); the in-memory index is
//! rebuilt from the sidecars at startup.

use crate::lol::Kda;
use crate::paths;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const DELETED_DIR: &str = "Recently Deleted";
pub const SESSION_FILE: &str = "session.mgr.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecEvent {
    /// Seconds into the recording.
    pub t: f64,
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi: Option<u8>,
    /// Capture window around `t`.
    pub before: f64,
    pub after: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub game: String,
    pub title: String,
    pub started_at: DateTime<Local>,
    #[serde(default)]
    pub ended_at: Option<DateTime<Local>>,
    #[serde(default)]
    pub champion: Option<String>,
    #[serde(default)]
    pub champion_key: Option<String>,
    #[serde(default)]
    pub kda: Option<Kda>,
    #[serde(default)]
    pub result: Option<String>,
    /// "client" when `result` came from the League client's match history (see `results`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_source: Option<String>,
    #[serde(default)]
    pub game_mode: Option<String>,
    #[serde(default)]
    pub capture_mode: String,
    /// Folder relative to the media root, forward slashes. Recomputed on load.
    #[serde(default)]
    pub rel_dir: String,
    #[serde(skip)]
    pub dir: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub id: String,
    pub session_id: String,
    /// match | session | highlight | manual | clip | screen | export | supercut | screenshot
    pub kind: String,
    pub title: String,
    pub file: String,
    #[serde(default)]
    pub thumb: Option<String>,
    pub created_at: DateTime<Local>,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub events: Vec<RecEvent>,
    /// Actions per second of video, index = second.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apm: Vec<u16>,
    #[serde(default)]
    pub source_id: Option<String>,
    /// Original file this recording was imported from (Outplayed or another library).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
    /// Content fingerprint of the imported source (see `ledger`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fingerprint: Option<String>,
    #[serde(default)]
    pub deleted_at: Option<DateTime<Local>>,
    /// Where a deleted recording came from, relative to the media root.
    #[serde(default)]
    pub original_dir: Option<String>,
    #[serde(default)]
    pub rel_dir: String,
    #[serde(skip)]
    pub dir: PathBuf,
}

impl Recording {
    pub fn path(&self) -> PathBuf {
        self.dir.join(&self.file)
    }

    pub fn sidecar(&self) -> PathBuf {
        sidecar_for(&self.dir, &self.file)
    }

    pub fn thumb_path(&self) -> Option<PathBuf> {
        self.thumb.as_ref().map(|t| self.dir.join(t))
    }
}

pub fn sidecar_for(dir: &Path, file: &str) -> PathBuf {
    let stem = Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or(file);
    dir.join(format!("{stem}.mgr.json"))
}

/// ("Match ", "03", " supercut") for the names the app numbers: "Match 03 supercut", "Session 01",
/// "Screen recording 12".
fn numbered(name: &str) -> Option<(&'static str, &str, &str)> {
    ["Match ", "Session ", "Screen recording "].into_iter().find_map(|kind| {
        let after = name.strip_prefix(kind)?;
        let digits = after.chars().take_while(char::is_ascii_digit).count();
        let (n, rest) = after.split_at(digits);
        (digits >= 2 && (rest.is_empty() || rest.starts_with(' '))).then_some((kind, n, rest))
    })
}

/// "Match 03 (2)" -> "Match 03": the suffix `unique_file` adds to avoid a clash.
fn strip_copy_suffix(stem: &str) -> &str {
    match stem.rfind(" (") {
        Some(i) if stem.ends_with(')') && stem[i + 2..stem.len() - 1].chars().all(|c| c.is_ascii_digit()) && stem.len() > i + 3 => &stem[..i],
        _ => stem,
    }
}

pub fn thumb_name(file: &str) -> String {
    let stem = Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or(file);
    format!("{stem}.thumb.jpg")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub sessions: Vec<Session>,
    pub recordings: Vec<Recording>,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Session>,
    recordings: HashMap<String, Recording>,
}

pub struct Library {
    root: RwLock<PathBuf>,
    inner: RwLock<Inner>,
}

fn rel(root: &Path, dir: &Path) -> String {
    dir.strip_prefix(root).unwrap_or(dir).to_string_lossy().replace('\\', "/")
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

impl Library {
    pub fn new(root: PathBuf) -> Library {
        let lib = Library { root: RwLock::new(root), inner: RwLock::default() };
        lib.reload();
        lib
    }

    pub fn root(&self) -> PathBuf {
        self.root.read().clone()
    }

    pub fn set_root(&self, root: PathBuf) {
        *self.root.write() = root;
        self.reload();
    }

    /// Rebuild the index from the sidecars on disk.
    pub fn reload(&self) {
        let root = self.root();
        let _ = std::fs::create_dir_all(&root);
        let mut inner = Inner::default();
        let mut stack = vec![(root.clone(), 0u32)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if path.is_dir() {
                    if depth < 3 && name != ".cache" && name != "Projects" {
                        stack.push((path, depth + 1));
                    }
                } else if name == SESSION_FILE {
                    if let Some(mut s) = read_json::<Session>(&path) {
                        s.dir = dir.clone();
                        s.rel_dir = rel(&root, &dir);
                        inner.sessions.insert(s.id.clone(), s);
                    }
                } else if name.ends_with(".mgr.json") {
                    if let Some(mut r) = read_json::<Recording>(&path) {
                        r.dir = dir.clone();
                        r.rel_dir = rel(&root, &dir);
                        if r.path().exists() {
                            inner.recordings.insert(r.id.clone(), r);
                        }
                    }
                }
            }
        }
        *self.inner.write() = inner;
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.inner.read();
        let mut sessions: Vec<Session> = inner.sessions.values().cloned().collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.started_at));
        let mut recordings: Vec<Recording> = inner.recordings.values().cloned().collect();
        recordings.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        Snapshot { sessions, recordings }
    }

    pub fn recording(&self, id: &str) -> Option<Recording> {
        self.inner.read().recordings.get(id).cloned()
    }

    pub fn session(&self, id: &str) -> Option<Session> {
        self.inner.read().sessions.get(id).cloned()
    }

    pub fn recordings_of(&self, session_id: &str) -> Vec<Recording> {
        let mut v: Vec<Recording> =
            self.inner.read().recordings.values().filter(|r| r.session_id == session_id && r.deleted_at.is_none()).cloned().collect();
        v.sort_by_key(|r| r.created_at);
        v
    }

    /// Next "Match NN" number for today.
    pub fn next_match_number(&self, game: &str) -> u32 {
        self.next_match_number_on(game, Local::now().date_naive())
    }

    pub fn next_match_number_on(&self, game: &str, day: chrono::NaiveDate) -> u32 {
        let n = self.inner.read().sessions.values().filter(|s| s.game == game && s.started_at.date_naive() == day).count();
        n as u32 + 1
    }

    /// Number a day's "Match NN" sessions 01, 02, ... in start order (imports can arrive out of order).
    /// Their recordings follow (see `sync_names`). Returns how many recordings were renamed.
    pub fn renumber_matches(&self, game: &str, day: chrono::NaiveDate) -> usize {
        let is_auto = |t: &str| t.strip_prefix("Match ").is_some_and(|n| n.len() >= 2 && n.chars().all(|c| c.is_ascii_digit()));
        let mut day_sessions: Vec<Session> = self
            .inner
            .read()
            .sessions
            .values()
            .filter(|s| s.game == game && s.started_at.date_naive() == day && is_auto(&s.title))
            .cloned()
            .collect();
        day_sessions.sort_by_key(|s| s.started_at);
        let mut renamed = 0;
        for (i, mut s) in day_sessions.into_iter().enumerate() {
            let title = format!("Match {:02}", i + 1);
            if s.title != title {
                s.title = title;
                let _ = self.save_session(&s);
            }
            renamed += self.sync_session_names(&s);
        }
        renamed
    }

    /// `sync_names` for every recording of a session; returns how many changed.
    fn sync_session_names(&self, s: &Session) -> usize {
        self.recordings_of(&s.id).iter().filter(|r| self.sync_names(s, r)).count()
    }

    /// Keep a recording's numbered name in step with its session: after "Match 02" became
    /// "Match 03", the recording "Match 02" / "Match 02 supercut" and its files ("Match 02.mp4",
    /// its thumbnail and sidecar) take the new number. Names the user chose and clips named after
    /// events ("Kill.mp4") are left alone. A file in use (playing, still being written) keeps its
    /// name until the next try. Returns whether anything changed.
    fn sync_names(&self, s: &Session, r: &Recording) -> bool {
        let mut next = r.clone();
        if let (Some((kind, n, rest)), Some((s_kind, s_n, ""))) = (numbered(&r.title), numbered(&s.title)) {
            if kind == s_kind && n != s_n {
                next.title = format!("{}{rest}", s.title);
            }
        }
        let stem = Path::new(&r.file).file_stem().and_then(|x| x.to_str()).unwrap_or_default();
        let wanted = crate::paths::sanitize(&next.title);
        // Only numbered names on both sides: a file the user named, or a recording they renamed, stays.
        let file_ok = numbered(stem).is_none() || numbered(&next.title).is_none() || strip_copy_suffix(stem) == wanted;
        if file_ok {
            return next.title != r.title && self.save_recording(&mut next).is_ok();
        }
        let ext = Path::new(&r.file).extension().and_then(|x| x.to_str()).unwrap_or("mp4");
        let free = |stem: &str| [format!("{stem}.{ext}"), thumb_name(stem), format!("{stem}.mgr.json")].iter().all(|f| !r.dir.join(f).exists());
        let Some(new_stem) = std::iter::once(wanted.clone()).chain((2..100).map(|k| format!("{wanted} ({k})"))).find(|x| free(x)) else { return false };
        next.file = format!("{new_stem}.{ext}");
        next.thumb = r.thumb.as_ref().map(|_| thumb_name(&next.file));
        // The new sidecar goes first: if anything stops halfway, a reload keeps whichever sidecar's
        // video exists, so the recording is never lost.
        if write_json(&next.sidecar(), &next).is_err() {
            return false;
        }
        if let Err(e) = std::fs::rename(r.path(), next.path()) {
            log::info!("{} keeps its name for now: {e}", r.path().display());
            let _ = std::fs::remove_file(next.sidecar());
            next.file = r.file.clone();
            next.thumb = r.thumb.clone();
            return next.title != r.title && self.save_recording(&mut next).is_ok();
        }
        if let (Some(old), Some(new)) = (r.thumb_path(), next.thumb_path()) {
            if std::fs::rename(old, new).is_err() {
                next.thumb = r.thumb.clone();
            }
        }
        let _ = std::fs::remove_file(r.sidecar());
        self.save_recording(&mut next).is_ok()
    }

    /// Results of matches converted from Outplayed, worked out again from their recordings with
    /// the rule of `import::match_result`. Imports before 2026-11-02 trusted Outplayed's win flag,
    /// which is wrong for most wins. Returns how many matches changed.
    pub fn repair_imported_results(&self) -> usize {
        let (sessions, recordings): (Vec<Session>, Vec<Recording>) = {
            let i = self.inner.read();
            (i.sessions.values().cloned().collect(), i.recordings.values().cloned().collect())
        };
        let mut changed = 0;
        for mut s in sessions {
            let recs: Vec<&Recording> = recordings.iter().filter(|r| r.session_id == s.id).collect();
            // Converted from Outplayed: every video carries its source fingerprint. Our own
            // recordings get their result from the game itself and are left alone.
            // A result looked up in the League client's history is authoritative.
            if recs.is_empty() || !recs.iter().all(|r| r.source_fingerprint.is_some()) || s.result_source.is_some() {
                continue;
            }
            let won = s.result.as_deref() == Some("win") || recs.iter().any(|r| r.events.iter().any(|e| e.kind == "victory"));
            let full = recs.iter().any(|r| r.kind == "match" || r.kind == "session");
            let result = if won { Some("win") } else if full { Some("loss") } else { None };
            if s.result.as_deref() != result {
                s.result = result.map(String::from);
                if self.save_session(&s).is_ok() {
                    changed += 1;
                }
            }
        }
        changed
    }

    /// `renumber_matches` for every day that has sessions, then names in step for the rest
    /// (sessions, screen recordings). Returns how many recordings were renamed.
    pub fn renumber_all(&self) -> usize {
        let days: HashSet<(String, chrono::NaiveDate)> = self.inner.read().sessions.values().map(|s| (s.game.clone(), s.started_at.date_naive())).collect();
        let mut renamed: usize = days.into_iter().map(|(game, day)| self.renumber_matches(&game, day)).sum();
        let sessions: Vec<Session> = self.inner.read().sessions.values().filter(|s| !s.title.starts_with("Match ")).cloned().collect();
        for s in sessions {
            renamed += self.sync_session_names(&s);
        }
        renamed
    }

    pub fn create_session(&self, game: &str, title: &str, capture_mode: &str) -> Result<Session> {
        self.create_session_at(game, title, capture_mode, Local::now())
    }

    /// Create a session dated `now` (imports use the original recording time).
    pub fn create_session_at(&self, game: &str, title: &str, capture_mode: &str, now: DateTime<Local>) -> Result<Session> {
        let root = self.root();
        let base = root.join(paths::sanitize(game)).join(now.format("%Y-%m-%d %H-%M-%S").to_string());
        let mut dir = base.clone();
        let mut n = 2;
        while dir.exists() {
            dir = PathBuf::from(format!("{} ({n})", base.display()));
            n += 1;
        }
        std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
        let s = Session {
            id: uuid::Uuid::new_v4().simple().to_string(),
            game: game.into(),
            title: title.into(),
            started_at: now,
            ended_at: None,
            champion: None,
            champion_key: None,
            kda: None,
            result: None,
            result_source: None,
            game_mode: None,
            capture_mode: capture_mode.into(),
            rel_dir: rel(&root, &dir),
            dir,
        };
        self.save_session(&s)?;
        Ok(s)
    }

    pub fn save_session(&self, s: &Session) -> Result<()> {
        write_json(&s.dir.join(SESSION_FILE), s)?;
        self.inner.write().sessions.insert(s.id.clone(), s.clone());
        Ok(())
    }

    /// Fill size/probe data and persist a new or changed recording.
    pub fn save_recording(&self, r: &mut Recording) -> Result<()> {
        let root = self.root();
        r.rel_dir = rel(&root, &r.dir);
        if let Ok(meta) = std::fs::metadata(r.path()) {
            r.size = meta.len();
        }
        write_json(&r.sidecar(), r)?;
        self.inner.write().recordings.insert(r.id.clone(), r.clone());
        Ok(())
    }

    /// Register a finished media file: probe it, make a thumbnail, write its sidecar.
    #[allow(clippy::too_many_arguments)] // mirrors the sidecar fields one to one
    pub fn add_file(&self, session: &Session, kind: &str, title: &str, file: &Path, events: Vec<RecEvent>, apm: Vec<u16>, source_id: Option<String>) -> Result<Recording> {
        let probe = crate::ffmpeg::probe(file).unwrap_or_default();
        let name = file.file_name().and_then(|n| n.to_str()).context("bad file name")?.to_string();
        let dir = file.parent().context("no parent")?.to_path_buf();
        let thumb = thumb_name(&name);
        let at = if kind == "match" || kind == "session" { probe.duration * 0.3 } else { probe.duration * 0.5 };
        let has_thumb = crate::ffmpeg::thumbnail(file, at, &dir.join(&thumb)).is_ok();
        let mut r = Recording {
            id: uuid::Uuid::new_v4().simple().to_string(),
            session_id: session.id.clone(),
            kind: kind.into(),
            title: title.into(),
            file: name,
            thumb: has_thumb.then_some(thumb),
            created_at: Local::now(),
            duration: probe.duration,
            size: 0,
            width: probe.width,
            height: probe.height,
            fps: probe.fps,
            codec: probe.codec,
            favorite: kind == "export",
            events,
            apm,
            source_id,
            imported_from: None,
            source_fingerprint: None,
            deleted_at: None,
            original_dir: None,
            rel_dir: String::new(),
            dir,
        };
        self.save_recording(&mut r)?;
        Ok(r)
    }

    /// Original paths of everything already imported (live or soft-deleted), lowercased.
    pub fn imported_sources(&self) -> std::collections::HashSet<String> {
        self.inner.read().recordings.values().filter_map(|r| r.imported_from.as_ref().map(|p| p.to_lowercase())).collect()
    }

    pub fn has_session(&self, id: &str) -> bool {
        self.inner.read().sessions.contains_key(id)
    }

    /// Recordings imported before the ledger existed (no fingerprint yet).
    pub fn imported_without_fingerprint(&self) -> Vec<Recording> {
        self.inner.read().recordings.values().filter(|r| r.imported_from.is_some() && r.source_fingerprint.is_none()).cloned().collect()
    }

    pub fn has_recording(&self, id: &str) -> bool {
        self.inner.read().recordings.contains_key(id)
    }

    pub fn update<F: FnOnce(&mut Recording)>(&self, id: &str, f: F) -> Result<Recording> {
        let mut r = self.recording(id).context(tr!("Recording not found", "找不到该录像"))?;
        f(&mut r);
        self.save_recording(&mut r)?;
        Ok(r)
    }

    fn files_of(r: &Recording) -> Vec<String> {
        let mut v = vec![r.file.clone()];
        if let Some(t) = &r.thumb {
            v.push(t.clone());
        }
        v.push(r.sidecar().file_name().unwrap().to_string_lossy().to_string());
        v
    }

    /// Soft delete: move the files into "Recently Deleted".
    pub fn soft_delete(&self, id: &str) -> Result<()> {
        let mut r = self.recording(id).context(tr!("Recording not found", "找不到该录像"))?;
        if r.deleted_at.is_some() {
            return Ok(());
        }
        let root = self.root();
        let dest = root.join(DELETED_DIR).join(&r.id);
        std::fs::create_dir_all(&dest)?;
        for f in Self::files_of(&r) {
            let _ = move_file(&r.dir.join(&f), &dest.join(&f));
        }
        r.original_dir = Some(rel(&root, &r.dir));
        r.deleted_at = Some(Local::now());
        r.dir = dest;
        self.save_recording(&mut r)?;
        self.prune_empty_session(&r.session_id);
        Ok(())
    }

    pub fn restore(&self, id: &str) -> Result<()> {
        let mut r = self.recording(id).context(tr!("Recording not found", "找不到该录像"))?;
        let Some(orig) = r.original_dir.clone() else { return Ok(()) };
        let root = self.root();
        let dest = root.join(&orig);
        std::fs::create_dir_all(&dest)?;
        let old_dir = r.dir.clone();
        for f in Self::files_of(&r) {
            let _ = move_file(&old_dir.join(&f), &dest.join(&f));
        }
        let _ = std::fs::remove_dir(&old_dir);
        r.deleted_at = None;
        r.original_dir = None;
        r.dir = dest.clone();
        // Bring the session back too if it was pruned.
        if self.session(&r.session_id).is_none() {
            if let Some(mut s) = read_json::<Session>(&dest.join(SESSION_FILE)) {
                s.dir = dest.clone();
                s.rel_dir = rel(&root, &dest);
                self.inner.write().sessions.insert(s.id.clone(), s);
            }
        }
        self.save_recording(&mut r)
    }

    /// Permanently delete a recording's files.
    pub fn purge(&self, id: &str) -> Result<()> {
        let r = self.recording(id).context(tr!("Recording not found", "找不到该录像"))?;
        for f in Self::files_of(&r) {
            let _ = std::fs::remove_file(r.dir.join(f));
        }
        if r.deleted_at.is_some() {
            let _ = std::fs::remove_dir(&r.dir);
        }
        self.inner.write().recordings.remove(id);
        self.prune_empty_session(&r.session_id);
        Ok(())
    }

    /// Hide sessions that have no live recordings left (their folder stays for restores).
    fn prune_empty_session(&self, session_id: &str) {
        if !self.recordings_of(session_id).is_empty() {
            return;
        }
        let has_deleted = self.inner.read().recordings.values().any(|r| r.session_id == session_id);
        let mut inner = self.inner.write();
        if let Some(s) = inner.sessions.get(session_id).cloned() {
            if !has_deleted {
                let _ = std::fs::remove_file(s.dir.join(SESSION_FILE));
                let _ = std::fs::remove_dir(&s.dir);
            }
            inner.sessions.remove(session_id);
        }
    }

    /// Permanently delete soft-deleted recordings older than `days`.
    pub fn purge_expired(&self, days: u32) -> usize {
        let cutoff = Local::now() - chrono::Duration::days(days as i64);
        let expired: Vec<String> = self
            .inner
            .read()
            .recordings
            .values()
            .filter(|r| r.deleted_at.is_some_and(|d| d < cutoff))
            .map(|r| r.id.clone())
            .collect();
        for id in &expired {
            let _ = self.purge(id);
        }
        expired.len()
    }

    pub fn total_size(&self) -> u64 {
        self.inner.read().recordings.values().filter(|r| r.deleted_at.is_none()).map(|r| r.size).sum()
    }

    /// Move the oldest non-favorite recordings to Recently deleted until the library fits in `limit` bytes.
    /// `protected` ids (used by protected editor projects) are skipped.
    pub fn enforce_limit(&self, limit: u64, protected: &[String]) -> usize {
        let mut total = self.total_size();
        if total <= limit {
            return 0;
        }
        let mut candidates: Vec<Recording> = self
            .inner
            .read()
            .recordings
            .values()
            .filter(|r| r.deleted_at.is_none() && !r.favorite && !protected.contains(&r.id))
            .cloned()
            .collect();
        candidates.sort_by_key(|r| self.played_at(r));
        let mut n = 0;
        for r in candidates {
            if total <= limit {
                break;
            }
            if self.soft_delete(&r.id).is_ok() {
                total = total.saturating_sub(r.size);
                n += 1;
            }
        }
        n
    }

    /// When the recorded game was played: its session's start (imports keep the original
    /// date), falling back to the file's creation time.
    fn played_at(&self, r: &Recording) -> DateTime<Local> {
        self.inner.read().sessions.get(&r.session_id).map(|s| s.started_at).unwrap_or(r.created_at)
    }

    /// Move recordings of matches played more than `days` ago to Recently deleted.
    /// Favorites and `protected` ids (protected editor projects) are kept.
    pub fn enforce_age(&self, days: u32, protected: &[String]) -> usize {
        let cutoff = Local::now() - chrono::Duration::days(days as i64);
        let live: Vec<Recording> =
            self.inner.read().recordings.values().filter(|r| r.deleted_at.is_none() && !r.favorite && !protected.contains(&r.id)).cloned().collect();
        let old: Vec<String> = live.into_iter().filter(|r| self.played_at(r) < cutoff).map(|r| r.id).collect();
        old.iter().filter(|id| self.soft_delete(id).is_ok()).count()
    }

    /// Finalize recordings left as `.part.mp4` by a crash or power loss. The fragmented file is
    /// playable up to the last complete fragment; it becomes a normal recording (without events).
    pub fn recover_partials(&self) -> usize {
        let root = self.root();
        let mut found = Vec::new();
        let mut stack = vec![(root.clone(), 0)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_dir() && depth < 3 && name != DELETED_DIR && name != ".cache" {
                    stack.push((p, depth + 1));
                } else if name.ends_with(".part.mp4") {
                    found.push(p);
                } else if name.ends_with(crate::import::PARTIAL_EXT) {
                    // An import conversion that never finished; the import redoes it.
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        let mut n = 0;
        for part in found {
            let dir = part.parent().unwrap().to_path_buf();
            let stem = part.file_name().unwrap().to_string_lossy().trim_end_matches(".part.mp4").to_string();
            let fin = paths::unique_file(&dir, &stem, "mp4");
            if crate::ffmpeg::remux_faststart(&part, &fin).is_err() {
                log::warn!("could not recover {}", part.display());
                continue;
            }
            let _ = std::fs::remove_file(&part);
            let session = read_json::<Session>(&dir.join(SESSION_FILE)).map(|mut s| {
                s.dir = dir.clone();
                s
            });
            let session = match session {
                Some(s) => s,
                None => continue,
            };
            let kind = if session.capture_mode == "session" { "session" } else if session.capture_mode == "screen" { "screen" } else { "match" };
            if self.add_file(&session, kind, &stem, &fin, vec![], vec![], None).is_ok() {
                self.inner.write().sessions.insert(session.id.clone(), session);
                log::info!("recovered {}", fin.display());
                n += 1;
            }
        }
        // Sessions that never produced anything (crash before the first frame).
        let empty: Vec<Session> = {
            let inner = self.inner.read();
            inner.sessions.values().filter(|s| !inner.recordings.values().any(|r| r.session_id == s.id)).cloned().collect()
        };
        for s in empty {
            let only_meta = std::fs::read_dir(&s.dir).map(|rd| rd.flatten().all(|e| e.file_name() == SESSION_FILE || e.file_name() == ".buffer")).unwrap_or(false);
            if only_meta {
                let _ = std::fs::remove_dir_all(&s.dir);
                self.inner.write().sessions.remove(&s.id);
            }
        }
        n
    }

    pub fn ensure_not_protected(&self, id: &str, protected: &[String]) -> Result<()> {
        if protected.iter().any(|p| p == id) {
            bail!(tr!("This video is used by a protected editor project. Turn off \"Protect project videos\" in that project to delete it.", "这个视频被一个受保护的剪辑项目使用。要删除它，请先在该项目中关闭“保护项目视频”。"));
        }
        Ok(())
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Rename, falling back to copy+delete across volumes.
pub fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.exists() {
        return Ok(());
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_names() {
        assert_eq!(numbered("Match 03 supercut"), Some(("Match ", "03", " supercut")));
        assert_eq!(numbered("Screen recording 12"), Some(("Screen recording ", "12", "")));
        assert_eq!(numbered("Match against Bob"), None);
        assert_eq!(numbered("Match 3"), None);
        assert_eq!(strip_copy_suffix("Match 03 (2)"), "Match 03");
        assert_eq!(strip_copy_suffix("Match 03"), "Match 03");
    }

    /// A day renumbered from "Match 02" to "Match 03": the match video, its supercut and their
    /// thumbnails and sidecars take the new number; event clips and user-chosen names stay.
    #[test]
    fn files_follow_the_match_number() {
        let root = std::env::temp_dir().join(format!("mgr-names-{}", uuid::Uuid::new_v4().simple()));
        let dir = root.join("League of Legends").join("2026-10-31 22-32-29");
        std::fs::create_dir_all(&dir).unwrap();
        let session = serde_json::json!({
            "id": "s1", "game": "League of Legends", "title": "Match 02", "startedAt": "2026-10-31T22:32:29-04:00", "captureMode": "match",
        });
        std::fs::write(dir.join(SESSION_FILE), session.to_string()).unwrap();
        let rec = |id: &str, title: &str, file: &str| {
            std::fs::write(dir.join(file), b"video").unwrap();
            std::fs::write(dir.join(thumb_name(file)), b"jpg").unwrap();
            let r = serde_json::json!({
                "id": id, "sessionId": "s1", "kind": "match", "title": title, "file": file, "thumb": thumb_name(file),
                "createdAt": "2026-10-31T22:52:36-04:00",
            });
            std::fs::write(sidecar_for(&dir, file), r.to_string()).unwrap();
        };
        rec("a", "Match 02", "Match 02.mp4");
        rec("b", "Match 02 supercut", "Match 02 supercut.mp4");
        rec("c", "Kill", "Kill.mp4");
        rec("d", "Pentakill vs Bob", "Match 02 (2).mp4");
        // An earlier match that day, recorded after the app restarted: this one becomes Match 03.
        let earlier = root.join("League of Legends").join("2026-10-31 22-03-05");
        std::fs::create_dir_all(&earlier).unwrap();
        let s0 = serde_json::json!({
            "id": "s0", "game": "League of Legends", "title": "Match 01", "startedAt": "2026-10-31T22:03:05-04:00", "captureMode": "match",
        });
        std::fs::write(earlier.join(SESSION_FILE), s0.to_string()).unwrap();
        let other = serde_json::json!({
            "id": "s9", "game": "League of Legends", "title": "Match 01", "startedAt": "2026-10-31T22:19:23-04:00", "captureMode": "match",
        });
        let mid = root.join("League of Legends").join("2026-10-31 22-19-23");
        std::fs::create_dir_all(&mid).unwrap();
        std::fs::write(mid.join(SESSION_FILE), other.to_string()).unwrap();

        let lib = Library::new(root.clone());
        let renamed = lib.renumber_all();
        let titles: HashMap<String, (String, String)> = lib.snapshot().recordings.into_iter().map(|r| (r.id, (r.title, r.file))).collect();
        assert_eq!(titles["a"], ("Match 03".into(), "Match 03.mp4".into()));
        assert_eq!(titles["b"], ("Match 03 supercut".into(), "Match 03 supercut.mp4".into()));
        assert_eq!(titles["c"], ("Kill".into(), "Kill.mp4".into()));
        assert_eq!(titles["d"], ("Pentakill vs Bob".into(), "Match 02 (2).mp4".into()));
        assert_eq!(renamed, 2);
        for f in ["Match 03.mp4", "Match 03.thumb.jpg", "Match 03.mgr.json", "Match 03 supercut.mp4", "Kill.mp4"] {
            assert!(dir.join(f).exists(), "{f} missing");
        }
        for f in ["Match 02.mp4", "Match 02.thumb.jpg", "Match 02.mgr.json", "Match 02 supercut.mgr.json"] {
            assert!(!dir.join(f).exists(), "{f} left behind");
        }
        // Survives a restart: the index rebuilt from disk agrees.
        let again = Library::new(root.clone());
        assert_eq!(again.recording("a").unwrap().file, "Match 03.mp4");
        assert_eq!(again.renumber_all(), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    /// A video another program holds open without delete sharing (a media player, Explorer's
    /// preview) cannot be renamed: it keeps its name, its recording stays intact, and the next try
    /// renames it once it is closed. (The app's own media server shares delete, so playing a video
    /// in the app never blocks this.)
    #[test]
    fn a_file_in_use_waits() {
        let root = std::env::temp_dir().join(format!("mgr-names-{}", uuid::Uuid::new_v4().simple()));
        let dir = root.join("League of Legends").join("2026-10-31 22-32-29");
        std::fs::create_dir_all(&dir).unwrap();
        let session = serde_json::json!({
            "id": "s1", "game": "League of Legends", "title": "Match 01", "startedAt": "2026-10-31T22:32:29-04:00", "captureMode": "match",
        });
        std::fs::write(dir.join(SESSION_FILE), session.to_string()).unwrap();
        std::fs::write(dir.join("Match 02.mp4"), b"video").unwrap();
        let r = serde_json::json!({ "id": "a", "sessionId": "s1", "kind": "match", "title": "Match 02", "file": "Match 02.mp4", "createdAt": "2026-10-31T22:52:36-04:00" });
        std::fs::write(dir.join("Match 02.mgr.json"), r.to_string()).unwrap();

        let lib = Library::new(root.clone());
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        let open = std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(dir.join("Match 02.mp4")).unwrap();
        lib.renumber_all();
        let r = lib.recording("a").unwrap();
        assert_eq!((r.title.as_str(), r.file.as_str()), ("Match 01", "Match 02.mp4"));
        assert!(dir.join("Match 02.mgr.json").exists() && !dir.join("Match 01.mgr.json").exists());
        assert_eq!(Library::new(root.clone()).recording("a").unwrap().title, "Match 01");
        drop(open);
        assert_eq!(lib.renumber_all(), 1);
        assert_eq!(lib.recording("a").unwrap().file, "Match 01.mp4");
        let _ = std::fs::remove_dir_all(root);
    }
}
