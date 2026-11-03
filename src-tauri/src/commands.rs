//! Tauri commands: the UI's whole API.

use crate::core::Core;
use crate::editor::{self, Project, ProjectClip};
use crate::ffmpeg::{self, Segment};
use crate::game::Cmd;
use crate::library::{RecEvent, Recording, Snapshot};
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, State};

type Core_<'a> = State<'a, Arc<Core>>;
type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    format!("{err:#}")
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(e)?.map_err(e)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    settings: Settings,
    media_url: String,
    version: String,
    status: crate::core::Status,
    /// The Windows display language as "en" | "zh-CN", for `general.language` = "system".
    system_language: &'static str,
    update: crate::update::UpdateState,
    /// The version this install was just updated from (first start after an update only).
    updated_from: Option<String>,
}

#[tauri::command]
pub fn bootstrap(core: Core_) -> Bootstrap {
    Bootstrap {
        settings: core.settings(),
        media_url: core.media_url.read().clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        status: core.status.read().clone(),
        system_language: crate::i18n::resolve("system"),
        update: crate::update::state(),
        updated_from: crate::update::updated_from(),
    }
}

#[tauri::command]
pub fn save_settings(core: Core_, settings: Settings) -> Res<()> {
    #[cfg(not(debug_assertions))]
    if core.settings().general.start_with_windows != settings.general.start_with_windows {
        use tauri_plugin_autostart::ManagerExt;
        let al = core.app.autolaunch();
        let result = if settings.general.start_with_windows { al.enable() } else { al.disable() };
        result.map_err(|err| tr!("Could not change the Windows startup entry: {err}", "无法更改 Windows 开机启动项：{err}"))?;
    }
    let storage_changed = serde_json::to_value(&core.settings().storage).ok() != serde_json::to_value(&settings.storage).ok();
    let language_changed = core.settings().general.language != settings.general.language;
    settings.save().map_err(e)?;
    // Every window gets the saved settings, so none keeps an older copy it could save back.
    let _ = core.app.emit("settings", &settings);
    if language_changed {
        crate::i18n::set(&settings.general.language);
        if let (Some(tray), Ok(menu)) = (core.app.tray_by_id("tray"), crate::tray_menu(&core.app)) {
            let _ = tray.set_menu(Some(menu));
        }
    }
    *core.settings.write() = settings;
    if storage_changed {
        let c = core.inner().clone();
        std::thread::spawn(move || c.housekeeping());
    }
    core.reregister_hotkeys();
    core.refresh_capture_affinity();
    core.send(Cmd::SettingsChanged);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Devices {
    audio: crate::capture::audio::AudioDevices,
    webcams: Vec<String>,
    monitors: Vec<crate::win::Monitor>,
    encoders: Vec<ffmpeg::EncoderInfo>,
}

#[tauri::command]
pub async fn list_devices() -> Res<Devices> {
    blocking(|| {
        Ok(Devices {
            audio: crate::capture::audio::list_devices(),
            webcams: crate::capture::list_webcams(),
            monitors: crate::win::monitors(),
            encoders: ffmpeg::encoders().to_vec(),
        })
    })
    .await
}

#[tauri::command]
pub fn get_library(core: Core_) -> Snapshot {
    core.library.snapshot()
}

#[tauri::command]
pub fn get_status(core: Core_) -> crate::core::Status {
    core.status.read().clone()
}

#[tauri::command]
pub fn set_favorite(core: Core_, id: String, favorite: bool) -> Res<Recording> {
    let r = core.library.update(&id, |r| r.favorite = favorite).map_err(e)?;
    core.library_changed();
    Ok(r)
}

#[tauri::command]
pub fn rename_recording(core: Core_, id: String, title: String) -> Res<Recording> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(tr!("Name cannot be empty", "名称不能为空"));
    }
    let r = core.library.update(&id, |r| r.title = title).map_err(e)?;
    core.library_changed();
    Ok(r)
}

#[tauri::command]
pub fn rename_session(core: Core_, id: String, title: String) -> Res<()> {
    let mut s = core.library.session(&id).ok_or("session not found")?;
    s.title = title.trim().to_string();
    core.library.save_session(&s).map_err(e)?;
    core.library_changed();
    Ok(())
}

#[tauri::command]
pub fn delete_recordings(core: Core_, ids: Vec<String>) -> Res<()> {
    let protected = editor::protected_ids(&core.library.root());
    for id in &ids {
        core.library.ensure_not_protected(id, &protected).map_err(e)?;
    }
    for id in ids {
        core.library.soft_delete(&id).map_err(e)?;
    }
    core.library_changed();
    Ok(())
}

#[tauri::command]
pub fn restore_recordings(core: Core_, ids: Vec<String>) -> Res<()> {
    for id in ids {
        core.library.restore(&id).map_err(e)?;
    }
    core.library_changed();
    Ok(())
}

#[tauri::command]
pub fn purge_recordings(core: Core_, ids: Vec<String>) -> Res<()> {
    for id in ids {
        core.library.purge(&id).map_err(e)?;
    }
    core.library_changed();
    Ok(())
}

#[tauri::command]
pub fn reveal_recording(core: Core_, id: String) -> Res<()> {
    let r = core.library.recording(&id).ok_or_else(|| tr!("Recording not found", "找不到该录像"))?;
    crate::win::reveal_in_explorer(&r.path()).map_err(e)
}

#[tauri::command]
pub fn open_media_folder(core: Core_) -> Res<()> {
    let root = core.library.root();
    std::fs::create_dir_all(&root).map_err(e)?;
    crate::win::hidden_command("explorer.exe").arg(root).spawn().map_err(e)?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    free: u64,
    total: u64,
    library: u64,
    /// good | low | critical
    level: String,
}

#[tauri::command]
pub fn disk_info(core: Core_) -> DiskInfo {
    let (free, total) = crate::win::disk_space(&core.library.root()).unwrap_or((0, 0));
    const GB: u64 = 1024 * 1024 * 1024;
    let level = if free < 10 * GB { "critical" } else if free < 50 * GB || (total > 0 && free * 10 < total) { "low" } else { "good" };
    DiskInfo { free, total, library: core.library.total_size(), level: level.into() }
}

/// Instant clip: stream-copied, so it is ready in a second.
#[tauri::command]
pub async fn quick_trim(core: Core_<'_>, id: String, start: f64, end: f64, title: String) -> Res<Recording> {
    let core = core.inner().clone();
    blocking(move || {
        let src = core.library.recording(&id).ok_or_else(|| anyhow::anyhow!("recording not found"))?;
        let session = core.library.session(&src.session_id).ok_or_else(|| anyhow::anyhow!("session not found"))?;
        let title = if title.trim().is_empty() { format!("{} clip", src.title) } else { title.trim().to_string() };
        let out = crate::paths::unique_file(&session.dir, &title, "mp4");
        ffmpeg::cut_copy(&src.path(), start, end, &out)?;
        // Keyframe snapping can start the cut slightly early; probe to place events exactly.
        let dur = ffmpeg::probe(&out).map(|p| p.duration).unwrap_or(end - start);
        let real_start = (end - dur).max(0.0).min(start);
        let events: Vec<RecEvent> = src
            .events
            .iter()
            .filter(|ev| ev.t >= real_start && ev.t <= end)
            .map(|ev| RecEvent { t: ev.t - real_start, ..ev.clone() })
            .collect();
        let apm = src.apm.iter().skip(real_start as usize).take((end - real_start).ceil() as usize).copied().collect();
        let r = core.library.add_file(&session, "clip", &title, &out, events, apm, Some(src.id.clone()))?;
        core.library_changed();
        Ok(r)
    })
    .await
}

fn render_codec(core: &Core, codec: &str) -> String {
    let pick = match codec {
        "av1" => "av1_nvenc",
        "hevc" => "hevc_nvenc",
        _ => "h264_nvenc",
    };
    let fam = ffmpeg::family(&core.settings.read().video.encoder);
    let want = match fam {
        "amf" => pick.replace("nvenc", "amf"),
        "qsv" => pick.replace("nvenc", "qsv"),
        _ => pick.to_string(),
    };
    ffmpeg::best_encoder(&want)
}

/// Instant supercut of one recording: the picked kinds of moment (kill tiers included, see
/// `supercut::key`), saved next to it.
#[tauri::command]
pub fn supercut(core: Core_, id: String, kinds: Vec<String>, before: f64, after: f64) -> Res<String> {
    let core = core.inner().clone();
    let src = core.library.recording(&id).ok_or_else(|| tr!("Recording not found", "找不到该录像"))?;
    let wins = crate::supercut::windows(&src, &kinds, before, after);
    if wins.is_empty() {
        return Err(tr!("No events of the selected types in this recording.", "这段录像里没有所选类型的事件。"));
    }
    let session = core.library.session(&src.session_id).ok_or_else(|| tr!("Recording not found", "找不到该录像"))?;
    let title = format!("{} supercut", src.title);
    let source = Some(src.id.clone());
    Ok(render_supercut(core, vec![(src, wins)], session, title, source))
}

/// Supercut across every match from `from` to `to` (inclusive, "YYYY-MM-DD"): the picked kinds
/// of moment from each match in turn, oldest first, in one video under Video editor.
#[tauri::command]
pub fn supercut_range(core: Core_, from: String, to: String, kinds: Vec<String>, before: f64, after: f64) -> Res<String> {
    let core = core.inner().clone();
    let day = |d: &str| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(e);
    let (from, to) = (day(&from)?, day(&to)?);
    let parts: Vec<(Recording, Vec<crate::supercut::Window>)> = crate::supercut::sources(&core.library, from, to)
        .into_iter()
        .map(|r| {
            let w = crate::supercut::windows(&r, &kinds, before, after);
            (r, w)
        })
        .filter(|(_, w)| !w.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(tr!("No moments of the selected kinds in these dates.", "这段日期里没有所选类型的时刻。"));
    }
    let session = match core.library.snapshot().sessions.into_iter().find(|s| s.game == "Video editor" && s.title == "Supercuts") {
        Some(s) => s,
        None => core.library.create_session("Video editor", "Supercuts", "export").map_err(e)?,
    };
    let (a, b) = (from.format("%Y-%m-%d").to_string(), to.format("%Y-%m-%d").to_string());
    let title = if from == to { tr!("Supercut {a}", "集锦 {a}") } else { tr!("Supercut {a} to {b}", "集锦 {a} 至 {b}") };
    Ok(render_supercut(core, parts, session, title, None))
}

/// Render supercut windows (from one or many recordings, in order) into `session` as a job.
fn render_supercut(core: Arc<Core>, parts: Vec<(Recording, Vec<crate::supercut::Window>)>, session: crate::library::Session, title: String, source: Option<String>) -> String {
    let job_title = crate::i18n::title(&title);
    let app = core.app.clone();
    core.clone().jobs.spawn(&app, "supercut", &job_title, move |ctx| {
        let out = crate::paths::unique_file(&session.dir, &title, "mp4");
        // The frame of the first source; others are fitted into it.
        let first = &parts[0].0;
        let segments: Vec<Segment> = parts
            .iter()
            .flat_map(|(r, ws)| ws.iter().map(move |w| Segment { file: r.path(), start: w.start, end: w.end, volume: 100.0, crop: "fit".into() }))
            .collect();
        let settings = core.settings();
        let spec = ffmpeg::RenderSpec {
            segments,
            width: first.width.max(2),
            height: first.height.max(2),
            fps: (first.fps.round() as u32).clamp(10, 120),
            encoder: ffmpeg::best_encoder(&settings.video.encoder),
            bitrate_kbps: settings.video.bitrate_kbps,
            master_volume: 100.0,
            watermark: settings.general.watermark,
            output: out.clone(),
        };
        ffmpeg::render_parts(&spec, &ctx.cancel, |p| ctx.progress(p))?;
        let mut events = Vec::new();
        let mut offset = 0.0;
        for w in parts.iter().flat_map(|(_, ws)| ws) {
            events.extend(w.events.iter().map(|ev| RecEvent { t: offset + (ev.t - w.start), ..ev.clone() }));
            offset += w.end - w.start;
        }
        let r = core.library.add_file(&session, "supercut", &title, &out, events, vec![], source)?;
        core.library_changed();
        Ok(Some(r.id))
    })
}

#[tauri::command]
pub fn cancel_job(core: Core_, id: String) {
    core.jobs.cancel(&id);
}

#[tauri::command]
pub async fn get_sprite(core: Core_<'_>, id: String) -> Res<Option<String>> {
    let core = core.inner().clone();
    blocking(move || {
        let Some(r) = core.library.recording(&id) else { return Ok(None) };
        if r.duration <= 0.0 || r.kind == "screenshot" {
            return Ok(None);
        }
        let rel = format!(".cache/sprites/{}.jpg", r.id);
        let path = core.library.root().join(&rel);
        if !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap())?;
            let count = (r.duration / 4.0).clamp(8.0, 120.0) as u32;
            ffmpeg::sprite(&r.path(), r.duration, count, &path)?;
        }
        Ok(Some(rel))
    })
    .await
}

#[tauri::command]
pub fn toggle_screen_recorder(core: Core_) {
    core.send(Cmd::ToggleScreenRecorder);
}

/// Champion names in Simplified Chinese, by Data Dragon key (empty when offline and uncached).
#[tauri::command]
pub async fn champion_names_zh() -> Res<std::collections::HashMap<String, String>> {
    blocking(|| Ok(crate::lol::art::names_zh().unwrap_or_default())).await
}

#[tauri::command]
pub async fn ensure_art(key: String) -> Res<bool> {
    blocking(move || Ok(crate::lol::art::ensure(&key))).await
}

// ---------- editor ----------

#[tauri::command]
pub fn list_projects(core: Core_) -> Vec<Project> {
    editor::list(&core.library.root())
}

#[tauri::command]
pub fn get_project(core: Core_, id: String) -> Res<Project> {
    editor::get(&core.library.root(), &id).map_err(e)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewClip {
    recording_id: String,
    start: f64,
    end: f64,
}

#[tauri::command]
pub fn create_project(core: Core_, clips: Vec<NewClip>) -> Res<Project> {
    let mut media: Vec<String> = Vec::new();
    let clips: Vec<ProjectClip> = clips
        .into_iter()
        .map(|c| {
            if !media.contains(&c.recording_id) {
                media.push(c.recording_id.clone());
            }
            ProjectClip { id: uuid::Uuid::new_v4().simple().to_string(), recording_id: c.recording_id, start: c.start, end: c.end, crop: "fit".into(), volume: 100.0 }
        })
        .collect();
    editor::create(&core.library.root(), media, clips).map_err(e)
}

#[tauri::command]
pub fn save_project(core: Core_, project: Project) -> Res<Project> {
    let mut p = project;
    editor::save(&core.library.root(), &mut p).map_err(e)?;
    Ok(p)
}

#[tauri::command]
pub fn delete_project(core: Core_, id: String) -> Res<()> {
    editor::delete(&core.library.root(), &id).map_err(e)
}

#[tauri::command]
pub fn export_project(core: Core_, id: String, codec: String) -> Res<String> {
    let core = core.inner().clone();
    let root = core.library.root();
    let project = editor::get(&root, &id).map_err(e)?;
    if project.clips.is_empty() {
        return Err(tr!("Add at least one clip to the timeline first.", "请先在时间轴上添加至少一个片段。"));
    }
    let mut segments = Vec::new();
    let mut first_size = None;
    for c in &project.clips {
        let r = core.library.recording(&c.recording_id).ok_or_else(|| tr!("A clip's source video was deleted.", "某个片段的源视频已被删除。"))?;
        first_size.get_or_insert((r.width, r.height));
        segments.push(Segment { file: r.path(), start: c.start, end: c.end, volume: c.volume, crop: c.crop.clone() });
    }
    let (w, h) = editor::frame_size(&project.aspect, first_size);
    let encoder = render_codec(&core, &codec);
    let app = core.app.clone();
    let title = project.name.clone();
    let job = core.clone().jobs.spawn(&app, "export", &crate::i18n::title(&title), move |ctx| {
        let session = match core.library.snapshot().sessions.into_iter().find(|s| s.game == "Video editor" && s.title == "Exports") {
            Some(s) => s,
            None => core.library.create_session("Video editor", "Exports", "export")?,
        };
        let out = crate::paths::unique_file(&session.dir, &title, "mp4");
        let settings = core.settings();
        let spec = ffmpeg::RenderSpec {
            segments,
            width: w,
            height: h,
            fps: settings.video.fps.clamp(30, 60),
            encoder,
            bitrate_kbps: settings.video.bitrate_kbps.max(8000),
            master_volume: project.volume,
            watermark: project.watermark || settings.general.watermark,
            output: out.clone(),
        };
        ffmpeg::render(&spec, &ctx.cancel, |p| ctx.progress(p))?;
        let r = core.library.add_file(&session, "export", &title, &out, vec![], vec![], None)?;
        Ok(Some(r.id))
    });
    Ok(job)
}

// ---------- storage ----------

fn copy_tree(from: &std::path::Path, to: &std::path::Path, total: u64, done: &mut u64, ctx: &crate::jobs::JobCtx, delete: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        if ctx.cancelled() {
            anyhow::bail!("cancelled");
        }
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if src.is_dir() {
            copy_tree(&src, &dst, total, done, ctx, delete)?;
            if delete {
                let _ = std::fs::remove_dir(&src);
            }
        } else {
            let moved = delete && std::fs::rename(&src, &dst).is_ok();
            if !moved {
                std::fs::copy(&src, &dst)?;
                if delete {
                    std::fs::remove_file(&src)?;
                }
            }
            *done += entry.metadata().map(|m| m.len()).unwrap_or(0);
            ctx.progress(*done as f64 / total.max(1) as f64);
        }
    }
    Ok(())
}

fn tree_size(p: &std::path::Path) -> u64 {
    std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| if e.path().is_dir() { tree_size(&e.path()) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }).sum())
        .unwrap_or(0)
}

/// Point the library at a new folder, moving existing media there when `move_media`.
#[tauri::command]
pub fn change_media_folder(core: Core_, path: String, move_media: bool) -> Res<String> {
    if core.status.read().state == "recording" {
        return Err(tr!("Wait until the current recording finishes.", "请等当前录制结束。"));
    }
    let core = core.inner().clone();
    let new_root = PathBuf::from(&path);
    let app = core.app.clone();
    Ok(core.clone().jobs.spawn(&app.clone(), "move", &tr!("Moving media folder", "正在移动媒体文件夹"), move |ctx| {
        let old_root = core.library.root();
        if move_media && old_root.exists() && old_root != new_root {
            let total = tree_size(&old_root);
            let mut done = 0;
            copy_tree(&old_root, &new_root, total, &mut done, ctx, true)?;
        }
        std::fs::create_dir_all(&new_root)?;
        let mut s = core.settings();
        s.storage.media_folder = new_root.to_string_lossy().into_owned();
        s.save()?;
        *core.settings.write() = s;
        core.library.set_root(new_root.clone());
        Ok(None)
    }))
}

#[tauri::command]
pub fn create_backup(core: Core_, dest: String) -> Res<String> {
    let core = core.inner().clone();
    let app = core.app.clone();
    Ok(core.clone().jobs.spawn(&app.clone(), "backup", &tr!("Backing up media", "正在备份媒体"), move |ctx| {
        let root = core.library.root();
        let target = PathBuf::from(dest).join(format!("Matt Game Recorder backup {}", chrono::Local::now().format("%Y-%m-%d %H-%M")));
        let total = tree_size(&root);
        let mut done = 0;
        copy_tree(&root, &target, total, &mut done, ctx, false)?;
        Ok(None)
    }))
}

#[tauri::command]
pub fn empty_recently_deleted(core: Core_) -> Res<()> {
    let ids: Vec<String> = core.library.snapshot().recordings.into_iter().filter(|r| r.deleted_at.is_some()).map(|r| r.id).collect();
    for id in ids {
        core.library.purge(&id).map_err(e)?;
    }
    core.library_changed();
    Ok(())
}

/// Look for a newer version now (Settings button).
#[tauri::command]
pub async fn check_update(core: Core_<'_>) -> Res<crate::update::UpdateState> {
    Ok(crate::update::check(&core).await)
}

/// Download and install the update found by the last check; the app restarts.
#[tauri::command]
pub async fn install_update(core: Core_<'_>) -> Res<()> {
    crate::update::install(&core).await.map_err(e)
}

#[tauri::command]
pub fn quit_app(core: Core_) {
    core.quit();
}

// ---------------------------------------------------------------------------------------------
// imports

#[tauri::command]
pub async fn scan_outplayed(core: Core_<'_>, folder: Option<String>) -> Res<crate::import::OutplayedScan> {
    let core = core.inner().clone();
    blocking(move || Ok(crate::import::scan_outplayed(&core, folder.as_deref()))).await
}

#[tauri::command]
pub fn import_outplayed(core: Core_, folder: String, include_deleted: Option<bool>, delete_sources: Option<bool>) -> Res<String> {
    let opts = crate::import::Options { include_deleted: include_deleted.unwrap_or(false), delete_sources: delete_sources.unwrap_or(false) };
    Ok(crate::import::start(core.inner(), "outplayed", PathBuf::from(folder), false, opts))
}

#[tauri::command]
pub fn import_library(core: Core_, folder: String) -> Res<String> {
    Ok(crate::import::start(core.inner(), "library", PathBuf::from(folder), false, Default::default()))
}

#[tauri::command]
pub fn list_jobs(core: Core_) -> Vec<crate::jobs::JobUpdate> {
    core.jobs.list()
}

/// Font families installed on this PC, for the Appearance page.
#[tauri::command]
pub async fn system_fonts() -> Vec<String> {
    static FONTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    tauri::async_runtime::spawn_blocking(|| FONTS.get_or_init(crate::win::font_families).clone()).await.unwrap_or_default()
}

const THEME_FILE_MAX: u64 = 256 * 1024;

fn theme_file_path(path: &str) -> Res<PathBuf> {
    let p = PathBuf::from(path);
    if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")) {
        return Err(tr!("Theme files are .json files.", "主题文件是 .json 文件。"));
    }
    Ok(p)
}

/// Read a theme file the user picked (.json, small).
#[tauri::command]
pub fn read_theme_file(path: String) -> Res<String> {
    let p = theme_file_path(&path)?;
    if std::fs::metadata(&p).map_err(e)?.len() > THEME_FILE_MAX {
        return Err(tr!("That file is too large to be a theme.", "这个文件太大，不是主题文件。"));
    }
    std::fs::read_to_string(&p).map_err(e)
}

/// Save a theme to the file the user picked.
#[tauri::command]
pub fn write_theme_file(path: String, contents: String) -> Res<()> {
    let p = theme_file_path(&path)?;
    if contents.len() as u64 > THEME_FILE_MAX {
        return Err(tr!("Theme is too large.", "主题太大。"));
    }
    std::fs::write(p, contents).map_err(e)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuAssist {
    on: bool,
    available: bool,
    /// Measured on this PC: how many times faster an import runs with CPU help.
    speedup: Option<f64>,
    measuring: bool,
}

fn cpu_assist_state(core: &Arc<Core>) -> CpuAssist {
    CpuAssist {
        on: crate::import::cpu_assist(),
        available: crate::import::cpu_assist_available(core),
        speedup: crate::import::cpu_speedup(core),
        measuring: crate::import::measuring_speed(),
    }
}

pub(crate) fn emit_cpu_assist(core: &Arc<Core>) -> CpuAssist {
    let state = cpu_assist_state(core);
    let _ = tauri::Emitter::emit(&core.app, "import-cpu-assist", &state);
    state
}

/// Current state; starts measuring this PC's encoder speeds the first time they are needed.
#[tauri::command]
pub fn import_cpu_assist(core: Core_) -> CpuAssist {
    let core = core.inner();
    let state = cpu_assist_state(core);
    if state.available && state.speedup.is_none() && !state.measuring {
        let c = core.clone();
        crate::import::measure_speed(core, move || {
            emit_cpu_assist(&c);
        });
        return cpu_assist_state(core);
    }
    state
}

/// Turn "Use the CPU as a second encoder" on or off; takes effect within a second, mid-import.
#[tauri::command]
pub fn set_import_cpu_assist(core: Core_, on: bool) -> CpuAssist {
    crate::import::set_cpu_assist(on);
    emit_cpu_assist(core.inner())
}

#[tauri::command]
pub fn pause_job(core: Core_, id: String, paused: bool) -> Res<()> {
    match core.jobs.set_paused(&core.app, &id, paused) {
        Some(kind) => {
            if kind == "import" {
                crate::import::set_pending_paused(paused);
            }
            Ok(())
        }
        None => Err(tr!("This job cannot be paused.", "此任务无法暂停。")),
    }
}

/// Move whole sessions (every recording in them) to Recently deleted.
#[tauri::command]
pub fn delete_sessions(core: Core_, ids: Vec<String>) -> Res<usize> {
    let protected = editor::protected_ids(&core.library.root());
    let recs: Vec<Recording> = ids.iter().flat_map(|id| core.library.recordings_of(id)).collect();
    for r in &recs {
        core.library.ensure_not_protected(&r.id, &protected).map_err(e)?;
    }
    for r in &recs {
        core.library.soft_delete(&r.id).map_err(e)?;
    }
    core.library_changed();
    Ok(recs.len())
}
