//! The game loop: detects League, starts/stops capture per the user's capture mode, turns live
//! game events into timeline marks and highlight clips, samples APM, and runs the manual
//! screen recorder. Runs on one thread, woken every second or by a command (hotkeys etc).

use crate::capture::audio::{AudioConfig, SystemSource};
use crate::capture::{self, CaptureOptions, Recorder, RingOptions, Target};
use crate::core::{Core, Hotkey};
use crate::input::InputMonitor;
use crate::library::{RecEvent, Session};
use crate::lol::{GameEvent, LiveClient, MatchTracker};
use crate::settings::{GameSettings, Settings};
use crate::platform;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Emitter;

pub const LOL_GAME: &str = "League of Legends";
/// The game process (in a match) and the client process (lobby, champion select).
#[cfg(windows)]
pub const GAME_EXE: &str = "League of Legends.exe";
#[cfg(windows)]
const CLIENT_EXE: &str = "LeagueClientUx.exe";
#[cfg(target_os = "macos")]
pub const GAME_EXE: &str = "LeagueofLegends";
#[cfg(target_os = "macos")]
const CLIENT_EXE: &str = "LeagueClientUx";
const SCREEN_GAME: &str = "Screen recorder";

pub enum Cmd {
    Hotkey(Hotkey),
    ToggleScreenRecorder,
    SettingsChanged,
    Shutdown,
}

struct Pending {
    event: RecEvent,
    pin: Option<u64>,
}

struct MatchSession {
    session: Session,
    /// match | highlights | manual | session
    mode: String,
    rec: Recorder,
    hwnd: isize,
    gs: GameSettings,
    tracker: MatchTracker,
    tracker_pid: Option<u32>,
    offset: Option<f64>,
    events: Vec<RecEvent>,
    pending: Vec<Pending>,
    clips_saved: u32,
    apm: Vec<u16>,
    input: Option<InputMonitor>,
    last_ult: f64,
    bookmark: Option<(f64, u64)>,
    game_end_at: Option<Instant>,
    ticks: u64,
    events_ok: bool,
    webcam_visible: bool,
    discord: Option<crate::discord::Presence>,
}

struct ScreenSession {
    session: Session,
    rec: Recorder,
    events: Vec<RecEvent>,
}

pub fn spawn(core: Arc<Core>) {
    let (tx, rx) = channel::<Cmd>();
    *core.cmd.lock() = Some(tx);
    std::thread::Builder::new()
        .name("game-loop".into())
        .spawn(move || {
            let live = LiveClient::new();
            let mut active: Option<MatchSession> = None;
            let mut screen: Option<ScreenSession> = None;
            let mut last_tick = Instant::now() - Duration::from_secs(5);
            let mut housekeeping_at = Instant::now();
            let mut retry_start: Option<(u32, Instant)> = None;
            loop {
                let wait = Duration::from_millis(1000).saturating_sub(last_tick.elapsed());
                match rx.recv_timeout(wait) {
                    Ok(Cmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                        if let Some(m) = active.take() {
                            finish_match(&core, m);
                        }
                        if let Some(s) = screen.take() {
                            finish_screen(&core, s);
                        }
                        core.app.exit(0);
                        return;
                    }
                    Ok(Cmd::Hotkey(h)) => handle_hotkey(&core, h, &mut active, &mut screen),
                    Ok(Cmd::ToggleScreenRecorder) => toggle_screen(&core, &mut screen),
                    Ok(Cmd::SettingsChanged) => {
                        retry_start = None;
                        if let Some(m) = active.as_mut() {
                            m.gs = core.settings().games.lol;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                }
                if last_tick.elapsed() < Duration::from_millis(950) {
                    continue;
                }
                last_tick = Instant::now();
                tick(&core, &live, &mut active, &mut screen, &mut retry_start);
                if housekeeping_at.elapsed() > Duration::from_secs(1800) {
                    housekeeping_at = Instant::now();
                    core.housekeeping();
                }
            }
        })
        .expect("game loop thread");
}

fn set_status(core: &Core, f: impl FnOnce(&mut crate::core::Status)) {
    f(&mut core.status.write());
    core.emit_status();
}

/// Game process whose match already ended and was saved. League stays open for a few seconds
/// after the end screen; this keeps that tail from starting a second recording.
static FINISHED_GAME_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn tick(core: &Arc<Core>, live: &LiveClient, active: &mut Option<MatchSession>, screen: &mut Option<ScreenSession>, retry_start: &mut Option<(u32, Instant)>) {
    use std::sync::atomic::Ordering;
    let settings = core.settings();
    let gs = settings.games.lol.clone();
    let procs = platform::processes();
    let game_pid = platform::find_process(&procs, GAME_EXE);
    let client_pid = platform::find_process(&procs, CLIENT_EXE);
    let finished = FINISHED_GAME_PID.load(Ordering::Relaxed);
    if finished != 0 && game_pid != Some(finished) {
        FINISHED_GAME_PID.store(0, Ordering::Relaxed);
    }
    let game_already_saved = finished != 0 && game_pid == Some(finished);

    if let Some(s) = screen.as_mut() {
        if let Some(err) = s.rec.failure() {
            log::error!("screen recorder failed: {err}");
            let s = screen.take().unwrap();
            finish_screen(core, s);
        } else {
            core.status.write().screen_elapsed = s.rec.now();
        }
    }

    match active.as_mut() {
        None => {
            let session_mode = gs.capture_mode == "session";
            let target_pid = if session_mode { client_pid.or(game_pid) } else { game_pid.filter(|_| !game_already_saved) };
            if gs.enabled {
                if let Some(pid) = target_pid {
                    if retry_start.is_some_and(|(failed_pid, at)| failed_pid == pid && at.elapsed() < Duration::from_secs(10)) {
                        return;
                    }
                    match start_match(core, &settings, pid, game_pid, session_mode) {
                        Ok(Some(m)) => {
                            *retry_start = None;
                            *active = Some(m);
                        }
                        Ok(None) => set_status(core, |s| s.state = "waiting".into()),
                        Err(e) => {
                            *retry_start = Some((pid, Instant::now()));
                            log::error!("could not start recording: {e:#}");
                            set_status(core, |s| {
                                s.state = "error".into();
                                s.message = Some(tr!("Could not start recording: {e:#}",
                                    zh: "无法开始录制：{e:#}", tw: "無法開始錄製：{e:#}",
                                    es: "No se pudo iniciar la grabación: {e:#}", fr: "Impossible de démarrer l'enregistrement : {e:#}",
                                    ja: "録画を開始できませんでした: {e:#}", ko: "녹화를 시작할 수 없습니다: {e:#}", ru: "Не удалось начать запись: {e:#}"));
                            });
                        }
                    }
                    return;
                }
            }
            let mut st = core.status.write();
            st.state = "idle".into();
            st.events_status = if game_pid.is_some() { "partial".into() } else { "idle".into() };
            st.mode = None;
            st.screen_recording = screen.is_some();
            drop(st);
            core.emit_status();
        }
        Some(m) => {
            m.ticks += 1;
            if let Some(err) = m.rec.failure() {
                log::error!("recording failed: {err}");
                core.notify(&tr!("Recording stopped",
                    zh: "录制已停止", tw: "錄製已停止",
                    es: "Grabación detenida", fr: "Enregistrement arrêté",
                    ja: "録画が停止しました", ko: "녹화 중지됨", ru: "Запись остановлена"), &tr!("The capture stopped unexpectedly. What was recorded so far has been saved.",
                    zh: "录制意外中断，已保存目前录到的内容。", tw: "錄製意外中斷，已儲存目前錄到的內容。",
                    es: "La captura se detuvo de forma inesperada. Lo grabado hasta ahora se ha guardado.", fr: "La capture s'est arrêtée de manière inattendue. Ce qui a été enregistré jusqu'ici est conservé.",
                    ja: "キャプチャが予期せず停止しました。ここまでの録画は保存されています。", ko: "캡처가 예기치 않게 중지되었습니다. 지금까지 녹화한 내용은 저장되었습니다.", ru: "Запись неожиданно прервалась. Всё записанное до этого момента сохранено."));
                let m = active.take().unwrap();
                finish_match(core, m);
                return;
            }
            let now = m.rec.now();

            // Live Client data only exists while the game process runs.
            if let Some(pid) = game_pid {
                if m.tracker_pid != Some(pid) {
                    // New game inside a full session: fresh tracker and clock offset.
                    m.tracker = MatchTracker::default();
                    m.tracker_pid = Some(pid);
                    m.offset = None;
                    if let Some(h) = platform::main_window_of(pid, Some("RiotWindowClass")) {
                        if let Some(inp) = &m.input {
                            inp.set_game_window(h);
                        }
                    }
                }
                if m.ticks % 5 == 1 || m.tracker.my_team.is_none() {
                    if let Some(ap) = live.active_player() {
                        m.tracker.update_active_player(&ap);
                    }
                    if let Some(pl) = live.player_list() {
                        m.tracker.update_players(&pl);
                    }
                }
                match live.game_stats() {
                    Some(stats) => {
                        m.events_ok = true;
                        m.tracker.game_mode = stats.get("gameMode").and_then(|v| v.as_str()).map(str::to_string);
                        let gt = stats.get("gameTime").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        if gt > 0.5 && m.rec.is_producing() {
                            let off = now - gt;
                            m.offset = Some(match m.offset {
                                Some(o) if (o - off).abs() < 3.0 => o * 0.9 + off * 0.1,
                                _ => off,
                            });
                        }
                        if m.offset.is_some() {
                            if let Some(evs) = live.events() {
                                for ge in m.tracker.ingest(&evs) {
                                    add_game_event(core, m, ge);
                                }
                            }
                        }
                    }
                    None => m.events_ok = m.ticks < 90 && m.events_ok,
                }
            }

            // APM and the ultimate heuristic.
            if let Some(inp) = &m.input {
                let actions = inp.take_actions();
                let sec = now.max(0.0) as usize;
                if m.apm.len() <= sec {
                    m.apm.resize(sec + 1, 0);
                }
                if m.gs.collect_apm {
                    m.apm[sec] = m.apm[sec].saturating_add(actions as u16);
                }
                if inp.take_ult_presses() > 0 && m.tracker.ult_level > 0 && m.gs.event_enabled("ultimate") && now - m.last_ult > 15.0 && game_pid.is_some() {
                    m.last_ult = now;
                    let tm = m.gs.timing_for("ultimate");
                    push_event(core, m, RecEvent { t: (now - 0.5).max(0.0), kind: "ultimate".into(), label: "Ultimate".into(), multi: None, before: tm.before, after: tm.after, ..Default::default() });
                }
            }

            process_pending(core, m, false);

            // End of match: game process gone, or the game reported its end.
            let ended = if m.mode == "session" {
                game_pid.is_none() && client_pid.is_none()
            } else if game_pid.is_none() {
                true
            } else if m.tracker.result.is_some() {
                let at = *m.game_end_at.get_or_insert_with(Instant::now);
                at.elapsed() > Duration::from_secs(4)
            } else {
                false
            };

            {
                let recent: Vec<u16> = m.apm.iter().rev().take(10).copied().collect();
                let apm = recent.iter().map(|&a| a as u32).sum::<u32>() * 60 / recent.len().max(1) as u32;
                let mut st = core.status.write();
                st.state = "recording".into();
                st.mode = Some(m.mode.clone());
                st.session_id = Some(m.session.id.clone());
                st.title = Some(m.session.title.clone());
                st.elapsed = now;
                st.champion = m.tracker.champion.clone();
                st.champion_key = m.tracker.champion_key.clone();
                st.kda = Some(m.tracker.kda.clone());
                st.events_status = if m.events_ok { "good".into() } else { "down".into() };
                st.events = m.events.clone();
                st.clips_saved = m.clips_saved;
                st.bookmarking = m.bookmark.is_some();
                st.screen_recording = screen.is_some();
                st.apm = apm;
                st.message = None;
            }
            core.emit_status();

            if ended {
                if let (Some(pid), false) = (game_pid, m.mode == "session") {
                    FINISHED_GAME_PID.store(pid, Ordering::Relaxed);
                }
                let m = active.take().unwrap();
                finish_match(core, m);
            }
        }
    }
}

/// Output size for a source of `w` x `h` under the video settings.
fn output_size(settings: &Settings, w: u32, h: u32, encoder: &str) -> ((u32, u32), bool) {
    let v = &settings.video;
    // Never upscale past the source.
    let target_h = if v.resolution == 0 { h } else { v.resolution.min(h) };
    let (mut ow, mut oh) = if v.force_16_9 {
        (target_h * 16 / 9, target_h)
    } else {
        let tw = (w as u64 * target_h as u64 / h.max(1) as u64) as u32;
        (tw, target_h)
    };
    // H.264 hardware encoders top out at 4096 px wide.
    if encoder.starts_with("h264") && ow > 4096 {
        oh = oh * 4096 / ow;
        ow = 4096;
    }
    ((ow / 2 * 2, oh / 2 * 2), v.force_16_9 && v.crop_mode == "stretch")
}

fn audio_config(settings: &Settings, game_pid: Option<u32>) -> AudioConfig {
    let a = &settings.audio;
    let system = a.system_enabled.then(|| match (a.game_only, game_pid) {
        (true, Some(pid)) => SystemSource::Process(pid),
        _ => SystemSource::Device(a.system_device.clone()),
    });
    AudioConfig {
        system,
        system_gain: a.system_volume as f32 / 100.0,
        mic: a.mic_enabled.then(|| a.mic_device.clone()),
        mic_gain: a.mic_volume as f32 / 100.0,
        mic_mono: a.mic_mono,
    }
}

fn start_match(core: &Arc<Core>, settings: &Settings, pid: u32, game_pid: Option<u32>, session_mode: bool) -> anyhow::Result<Option<MatchSession>> {
    let gs = settings.games.lol.clone();
    let class = if session_mode && game_pid != Some(pid) { None } else { Some("RiotWindowClass") };
    let Some(hwnd) = platform::main_window_of(pid, class).or_else(|| platform::main_window_of(pid, None)) else { return Ok(None) };
    if platform::is_minimized(hwnd) {
        return Ok(None);
    }
    let Some((ww, wh)) = platform::window_size(hwnd) else { return Ok(None) };
    // League creates its window tiny and resizes it while loading; size the capture from the real one.
    if !session_mode && (ww < 640 || wh < 360) {
        return Ok(None);
    }
    let encoder = crate::ffmpeg::best_encoder(&settings.video.encoder);

    let mode = gs.capture_mode.clone();
    let n = core.library.next_match_number(LOL_GAME);
    let title = if session_mode { format!("Session {n:02}") } else { format!("Match {n:02}") };
    let session = core.library.create_session(LOL_GAME, &title, &mode)?;

    let (target, (size, stretch)) = if session_mode {
        let mon = platform::monitor_of_window(hwnd);
        let m = platform::monitors().into_iter().find(|m| m.handle == mon);
        let (mw, mh) = m.map(|m| (m.width, m.height)).unwrap_or((ww, wh));
        (Target::Monitor(mon), output_size(settings, mw, mh, &encoder))
    } else {
        (Target::Window(hwnd), output_size(settings, ww, wh, &encoder))
    };

    let full = mode == "match" || mode == "session";
    let keep = ["victory", "kill", "death", "assist", "ultimate", "baron", "dragon", "herald", "turret", "inhibitor"]
        .iter()
        .map(|k| gs.timing_for(k).before)
        .chain([gs.hotkeys.place_before, gs.hotkeys.bookmark_toggle_before])
        .fold(0.0, f64::max)
        + 15.0;
    let rec = Recorder::start(CaptureOptions {
        target,
        fps: settings.video.fps,
        size: Some(size),
        stretch,
        cursor: gs.capture_cursor,
        border: false,
        exclude_self: !settings.general.capture_own_windows,
        encoder,
        encoder_preset: settings.video.encoder_preset.clone(),
        bitrate_kbps: settings.video.bitrate_kbps,
        audio: audio_config(settings, game_pid),
        webcam: settings.webcam.enabled.then(|| settings.webcam.device.clone()).filter(|d| !d.is_empty()),
        file: full.then(|| session.dir.join(format!("{}.mp4", crate::paths::sanitize(&title)))),
        ring: (!full).then(|| RingOptions { in_ram: settings.video.buffer != "disk", temp_dir: session.dir.join(".buffer"), keep_secs: keep }),
    })?;

    let needs_input = gs.collect_apm || gs.event_enabled("ultimate");
    let input = needs_input.then(|| InputMonitor::start(hwnd, &gs.hotkeys.ultimate_key, gs.collect_apm));
    core.register_hotkeys(true);
    if gs.dashboard_on_secondary && platform::monitors().len() > 1 {
        core.open_dashboard(hwnd);
    }
    let n = &settings.notifications;
    let what = match mode.as_str() {
        "match" => tr!("Recording the full match",
            zh: "正在录制整场对局", tw: "正在錄製整場對局",
            es: "Grabando la partida completa", fr: "Enregistrement de la partie complète",
            ja: "試合全体を録画中", ko: "게임 전체 녹화 중", ru: "Запись всего матча"),
        "session" => tr!("Recording the full session",
            zh: "正在录制整个会话", tw: "正在錄製整個時段",
            es: "Grabando la sesión completa", fr: "Enregistrement de la session complète",
            ja: "セッション全体を録画中", ko: "세션 전체 녹화 중", ru: "Запись всей сессии"),
        "highlights" => tr!("Capturing highlights",
            zh: "正在捕捉高光", tw: "正在擷取精華",
            es: "Capturando momentos destacados", fr: "Capture des temps forts",
            ja: "ハイライトをキャプチャ中", ko: "하이라이트 캡처 중", ru: "Запись лучших моментов"),
        _ => tr!("Ready for manual captures",
            zh: "可以手动捕捉", tw: "可以手動擷取",
            es: "Listo para capturas manuales", fr: "Prêt pour les captures manuelles",
            ja: "手動キャプチャの準備完了", ko: "수동 캡처 준비됨", ru: "Готово к ручной записи"),
    };
    if (session_mode && n.session_start) || (!session_mode && n.match_start) {
        let shown = crate::i18n::title(&title);
        core.notify(&tr!("{shown} started",
            zh: "{shown} 已开始", tw: "{shown} 已開始",
            es: "{shown} ha empezado", fr: "{shown} a commencé",
            ja: "{shown} を開始しました", ko: "{shown} 시작됨", ru: "{shown}: начало"), &what);
    }
    let discord = (settings.general.discord_presence && !settings.general.discord_app_id.is_empty())
        .then(|| crate::discord::Presence::start(&settings.general.discord_app_id, &what))
        .flatten();
    log::info!("recording {title} ({mode}) at {}x{}", size.0, size.1);
    core.library_changed();
    Ok(Some(MatchSession {
        session,
        webcam_visible: settings.webcam.mode == "always",
        mode,
        rec,
        hwnd,
        gs,
        tracker: MatchTracker::default(),
        tracker_pid: None,
        offset: None,
        events: Vec::new(),
        pending: Vec::new(),
        clips_saved: 0,
        apm: Vec::new(),
        input,
        last_ult: -100.0,
        bookmark: None,
        game_end_at: None,
        ticks: 0,
        events_ok: false,
        discord,
    }))
}

fn add_game_event(core: &Core, m: &mut MatchSession, ge: GameEvent) {
    if !m.gs.event_enabled(&ge.kind) {
        return;
    }
    let Some(offset) = m.offset else { return };
    let t = ge.game_time + offset;
    if t < 0.0 {
        // Happened before the recording started (e.g. the app was opened mid-game).
        return;
    }
    // A multikill becomes the streak's one mark: it takes the kill it was reported with, then the
    // streak's earlier kills (see `merge_multikills`). Clips waiting to be cut follow.
    if ge.kind == "multikill" {
        let tm = m.gs.timing_for("kill");
        let mut ev = RecEvent { t, kind: "multikill".into(), label: ge.label, multi: ge.multi, before: tm.before, after: tm.after, ..Default::default() };
        if let Some(i) = m.events.iter().rposition(|e| e.kind == "kill" && (e.t - t).abs() < 2.0) {
            let k = m.events.remove(i);
            ev.t = k.t;
            m.pending.retain(|p| !(p.event.kind == "kill" && (p.event.t - k.t).abs() < 0.01));
        }
        let before: Vec<(f64, String)> = m.events.iter().map(|e| (e.t, e.kind.clone())).collect();
        m.events.push(ev);
        m.events.sort_by(|a, b| a.t.total_cmp(&b.t));
        crate::library::merge_multikills(&mut m.events);
        // Drop pending clips of the kills that were merged away, and queue the streak's clip.
        let kept: Vec<(f64, String)> = m.events.iter().map(|e| (e.t, e.kind.clone())).collect();
        let gone: Vec<(f64, String)> = before.into_iter().filter(|b| !kept.iter().any(|k| (k.0 - b.0).abs() < 0.01 && k.1 == b.1)).collect();
        m.pending.retain(|p| !gone.iter().any(|g| (g.0 - p.event.t).abs() < 0.01 && g.1 == p.event.kind) && p.event.kind != "multikill");
        if let Some(streak) = m.events.iter().rev().find(|e| e.kind == "multikill").cloned() {
            if m.mode == "highlights" {
                m.pending.push(Pending { event: streak.clone(), pin: None });
            }
            let _ = core.app.emit("live-event", streak);
        }
        return;
    }
    let tm = m.gs.timing_for(&ge.kind);
    push_event(core, m, RecEvent { t, kind: ge.kind, label: ge.label, multi: ge.multi, before: tm.before, after: tm.after, champion: ge.champion, span: None });
}

fn push_event(core: &Core, m: &mut MatchSession, ev: RecEvent) {
    if m.mode == "highlights" || ((m.mode == "manual") && ev.kind == "bookmark") {
        m.pending.push(Pending { event: ev.clone(), pin: None });
    }
    m.events.push(ev.clone());
    m.events.sort_by(|a, b| a.t.total_cmp(&b.t));
    let _ = core.app.emit("live-event", ev);
}

/// Event importance, used to title a clip after its best moment.
fn rank(e: &RecEvent) -> u32 {
    match e.kind.as_str() {
        "multikill" => 100 + e.multi.unwrap_or(2) as u32,
        "kill" => 90,
        "baron" => 80,
        "dragon" => 75,
        "herald" => 70,
        "inhibitor" => 65,
        "turret" => 60,
        "assist" => 50,
        "victory" => 45,
        "ultimate" => 40,
        "death" => 30,
        _ => 10,
    }
}

fn kind_title(e: &RecEvent) -> String {
    match e.kind.as_str() {
        "multikill" => e.label.clone(),
        "bookmark" => "Manual capture".into(),
        k => {
            let mut c = k.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// Cut highlight clips whose capture window has fully elapsed (or everything, when `force`).
fn process_pending(core: &Core, m: &mut MatchSession, force: bool) {
    if m.pending.is_empty() {
        return;
    }
    m.pending.sort_by(|a, b| a.event.t.total_cmp(&b.event.t));
    let now = m.rec.now();
    let mut groups: Vec<Vec<Pending>> = Vec::new();
    for p in m.pending.drain(..) {
        let start = p.event.t - p.event.before;
        match groups.last_mut() {
            Some(g) if start <= g.iter().map(|q| q.event.t + q.event.after).fold(f64::MIN, f64::max) + 1.0 => g.push(p),
            _ => groups.push(vec![p]),
        }
    }
    for g in groups {
        let a = g.iter().map(|p| p.event.t - p.event.before).fold(f64::MAX, f64::min).max(0.0);
        let b = g.iter().map(|p| p.event.t + p.event.after).fold(f64::MIN, f64::max);
        // A kill may still become part of a multikill streak: wait out the streak window first.
        let grace = if g.iter().any(|p| p.event.kind == "kill" || p.event.kind == "multikill") { 11.0 } else { 3.0 };
        if !force && b + grace > now {
            m.pending.extend(g);
            continue;
        }
        let best = g.iter().max_by_key(|p| rank(&p.event)).map(|p| p.event.clone()).unwrap();
        let title = kind_title(&best);
        let file = crate::paths::unique_file(&m.session.dir, &title, "mp4");
        match m.rec.extract(a, b.max(a + 1.0), &file) {
            Ok((start, _end)) => {
                let events: Vec<RecEvent> = g.iter().map(|p| RecEvent { t: (p.event.t - start).max(0.0), ..p.event.clone() }).collect();
                let apm: Vec<u16> = m.apm.iter().skip(start.max(0.0) as usize).take((b - start).ceil() as usize + 1).copied().collect();
                let kind = if best.kind == "bookmark" { "manual" } else { "highlight" };
                match core.library.add_file(&m.session, kind, &title, &file, events, apm, None) {
                    Ok(_) => {
                        m.clips_saved += 1;
                        let n = &core.settings.read().notifications;
                        let notify = if kind == "manual" { n.manual_capture } else { n.game_event_capture };
                        if notify {
                            let shown = crate::i18n::title(&title);
                            core.notify(&tr!("{shown} saved",
                                zh: "{shown} 已保存", tw: "{shown} 已儲存",
                                es: "{shown} guardado", fr: "{shown} enregistré",
                                ja: "{shown} を保存しました", ko: "{shown} 저장됨", ru: "{shown}: сохранено"), &crate::i18n::title(&m.session.title));
                        }
                        core.library_changed();
                    }
                    Err(e) => log::error!("could not register clip: {e:#}"),
                }
            }
            Err(e) => log::warn!("clip {title} skipped: {e:#}"),
        }
        for p in &g {
            if let Some(pin) = p.pin {
                m.rec.unpin(pin);
            }
        }
    }
}

fn finish_match(core: &Arc<Core>, mut m: MatchSession) {
    set_status(core, |s| {
        s.state = "saving".into();
        let shown = crate::i18n::title(&m.session.title);
        s.message = Some(tr!("Saving {shown}...",
            zh: "正在保存 {shown}...", tw: "正在儲存 {shown}...",
            es: "Guardando {shown}...", fr: "Enregistrement de {shown}...",
            ja: "{shown} を保存中...", ko: "{shown} 저장 중...", ru: "Сохранение: {shown}..."));
    });
    if let Some((start, pin)) = m.bookmark.take() {
        end_manual_capture(&mut m, start, pin);
    }
    process_pending(core, &mut m, true);
    m.input = None;
    core.register_hotkeys(false);
    core.close_dashboard();
    drop(m.discord.take());

    let MatchSession { mut session, rec, tracker, events, apm, mode, clips_saved, .. } = m;
    let finished = rec.stop();
    session.ended_at = Some(chrono::Local::now());
    session.champion = tracker.champion.clone();
    session.champion_key = tracker.champion_key.clone();
    session.kda = tracker.champion.as_ref().map(|_| tracker.kda.clone());
    session.result = tracker.result.clone();
    session.game_mode = tracker.game_mode.clone();
    let _ = core.library.save_session(&session);

    let mut saved_any = clips_saved > 0;
    match finished {
        Ok(f) => {
            if let Some(file) = f.file {
                match core.library.add_file(&session, if mode == "session" { "session" } else { "match" }, &session.title, &file, events, apm, None) {
                    Ok(_) => saved_any = true,
                    Err(e) => log::error!("could not register recording: {e:#}"),
                }
            }
        }
        Err(e) => log::error!("stopping capture failed: {e:#}"),
    }
    if !saved_any {
        // Manual mode without captures: leave nothing behind.
        let _ = std::fs::remove_dir_all(&session.dir);
        core.library.reload();
    }
    if let Some(key) = session.champion_key.clone() {
        std::thread::spawn(move || {
            crate::lol::art::ensure(&key);
        });
    }

    let settings = core.settings();
    if saved_any && settings.notifications.match_end {
        let kda = session.kda.as_ref().map(|k| format!("KDA {}/{}/{}", k.kills, k.deaths, k.assists)).unwrap_or_default();
        let res = match session.result.as_deref() {
            Some("win") => tr!("Victory. ", zh: "胜利。", tw: "勝利。", es: "Victoria. ", fr: "Victoire. ", ja: "勝利。", ko: "승리. ", ru: "Победа. "),
            Some("loss") => tr!("Defeat. ", zh: "失败。", tw: "失敗。", es: "Derrota. ", fr: "Défaite. ", ja: "敗北。", ko: "패배. ", ru: "Поражение. "),
            _ => String::new(),
        };
        let shown = crate::i18n::title(&session.title);
        core.notify(&tr!("{shown} saved",
            zh: "{shown} 已保存", tw: "{shown} 已儲存",
            es: "{shown} guardado", fr: "{shown} enregistré",
            ja: "{shown} を保存しました", ko: "{shown} 저장됨", ru: "{shown}: сохранено"), &format!("{res}{kda}"));
    }
    set_status(core, |s| {
        *s = crate::core::Status { state: "idle".into(), events_status: "idle".into(), ..Default::default() };
    });
    core.housekeeping();
    core.library_changed();
    if saved_any && settings.games.lol.open_app_on_end {
        core.show_main();
        let _ = core.app.emit("open-session", session.id.clone());
    }
}

fn handle_hotkey(core: &Arc<Core>, h: Hotkey, active: &mut Option<MatchSession>, screen: &mut Option<ScreenSession>) {
    match h {
        Hotkey::ScreenToggle => toggle_screen(core, screen),
        Hotkey::ScreenBookmark => {
            if let Some(s) = screen.as_mut() {
                let sr = core.settings().screen_recorder;
                let t = s.rec.now();
                s.events.push(RecEvent { t, kind: "bookmark".into(), label: "Bookmark".into(), multi: None, before: sr.bookmark_before, after: sr.bookmark_after, ..Default::default() });
            }
        }
        Hotkey::ToggleApp => core.toggle_main_on_top(),
        _ => {}
    }
    let Some(m) = active.as_mut() else { return };
    match h {
        Hotkey::PlaceBookmark => {
            let hk = &m.gs.hotkeys;
            let ev = RecEvent { t: m.rec.now(), kind: "bookmark".into(), label: "Bookmark".into(), multi: None, before: hk.place_before, after: hk.place_after, ..Default::default() };
            push_event(core, m, ev);
        }
        Hotkey::BookmarkToggle => match m.bookmark.take() {
            None => {
                let now = m.rec.now();
                let pin = m.rec.pin(now - m.gs.hotkeys.bookmark_toggle_before);
                m.bookmark = Some((now, pin));
                if core.settings.read().notifications.manual_capture {
                    core.notify(&tr!("Manual capture started",
                        zh: "手动捕捉已开始", tw: "手動擷取已開始",
                        es: "Captura manual iniciada", fr: "Capture manuelle lancée",
                        ja: "手動キャプチャを開始しました", ko: "수동 캡처 시작됨", ru: "Ручная запись начата"), &tr!("Press the hotkey again to stop.",
                        zh: "再按一次快捷键停止。", tw: "再按一次快速鍵停止。",
                        es: "Pulsa el atajo otra vez para detenerla.", fr: "Appuyez de nouveau sur le raccourci pour l'arrêter.",
                        ja: "もう一度ホットキーを押すと停止します。", ko: "단축키를 한 번 더 누르면 중지됩니다.", ru: "Нажмите горячую клавишу ещё раз, чтобы остановить."));
                }
            }
            Some((start, pin)) => end_manual_capture(m, start, pin),
        },
        Hotkey::Screenshot => {
            let file = crate::paths::unique_file(&m.session.dir, "Screenshot", "png");
            match capture::screenshot(&Target::Window(m.hwnd), &file) {
                Ok(()) => {
                    let _ = core.library.add_file(&m.session, "screenshot", "Screenshot", &file, vec![], vec![], None);
                    core.library_changed();
                }
                Err(e) => log::warn!("screenshot failed: {e:#}"),
            }
        }
        Hotkey::ToggleWebcam => {
            m.webcam_visible = !m.webcam_visible;
            m.rec.set_webcam_visible(m.webcam_visible);
        }
        Hotkey::ToggleDashboard => core.toggle_dashboard(m.hwnd),
        Hotkey::DashboardLayout => {
            let _ = core.app.emit("dashboard-layout", ());
        }
        _ => {}
    }
}

/// A manual capture (started by the hotkey at `start`) ends now: it is marked on the timeline, and
/// outside full recordings its clip is queued, holding the buffer from `pin` until it is cut.
fn end_manual_capture(m: &mut MatchSession, start: f64, pin: u64) {
    let after = (m.rec.now() - start).max(1.0);
    let ev = RecEvent { t: start, kind: "bookmark".into(), label: "Manual capture".into(), before: m.gs.hotkeys.bookmark_toggle_before, after, ..Default::default() };
    m.events.push(ev.clone());
    if m.mode == "match" || m.mode == "session" {
        m.rec.unpin(pin);
    } else {
        m.pending.push(Pending { event: ev, pin: Some(pin) });
    }
}

fn toggle_screen(core: &Arc<Core>, screen: &mut Option<ScreenSession>) {
    if let Some(s) = screen.take() {
        finish_screen(core, s);
        return;
    }
    let settings = core.settings();
    let sr = &settings.screen_recorder;
    let monitors = platform::monitors();
    let Some(mon) = monitors.get(sr.display as usize).or_else(|| monitors.first()).cloned() else { return };
    let encoder = crate::ffmpeg::best_encoder(&settings.video.encoder);
    let (size, stretch) = output_size(&settings, mon.width, mon.height, &encoder);
    let n = core.library.next_match_number(SCREEN_GAME);
    let title = format!("Screen recording {n:02}");
    let result = core.library.create_session(SCREEN_GAME, &title, "screen").and_then(|session| {
        let rec = Recorder::start(CaptureOptions {
            target: Target::Monitor(mon.handle),
            fps: settings.video.fps,
            size: Some(size),
            stretch,
            cursor: sr.capture_cursor,
            border: sr.show_border,
            exclude_self: false,
            encoder,
            encoder_preset: settings.video.encoder_preset.clone(),
            bitrate_kbps: settings.video.bitrate_kbps,
            audio: audio_config(&settings, None),
            webcam: None,
            file: Some(session.dir.join(format!("{title}.mp4"))),
            ring: None,
        })?;
        Ok(ScreenSession { session, rec, events: vec![] })
    });
    match result {
        Ok(s) => {
            *screen = Some(s);
            set_status(core, |st| {
                st.screen_recording = true;
                st.screen_elapsed = 0.0;
            });
            if settings.notifications.manual_capture {
                let key = &sr.hotkey_toggle;
                core.notify(&tr!("Screen recording started",
                    zh: "屏幕录制已开始", tw: "螢幕錄製已開始",
                    es: "Grabación de pantalla iniciada", fr: "Enregistrement d'écran lancé",
                    ja: "画面録画を開始しました", ko: "화면 녹화 시작됨", ru: "Запись экрана начата"), &tr!("Press {key} to stop.",
                    zh: "按 {key} 停止。", tw: "按 {key} 停止。",
                    es: "Pulsa {key} para detenerla.", fr: "Appuyez sur {key} pour l'arrêter.",
                    ja: "{key} を押すと停止します。", ko: "{key}를 누르면 중지됩니다.", ru: "Нажмите {key}, чтобы остановить."));
            }
        }
        Err(e) => {
            log::error!("screen recorder failed to start: {e:#}");
            core.notify(&tr!("Screen recorder",
                zh: "屏幕录制", tw: "螢幕錄製",
                es: "Grabadora de pantalla", fr: "Enregistreur d'écran",
                ja: "画面録画", ko: "화면 녹화", ru: "Запись экрана"), &tr!("Could not start: {e:#}",
                zh: "无法开始：{e:#}", tw: "無法開始：{e:#}",
                es: "No se pudo iniciar: {e:#}", fr: "Impossible de démarrer : {e:#}",
                ja: "開始できませんでした: {e:#}", ko: "시작할 수 없습니다: {e:#}", ru: "Не удалось запустить: {e:#}"));
        }
    }
}

fn finish_screen(core: &Arc<Core>, s: ScreenSession) {
    let ScreenSession { mut session, rec, events } = s;
    set_status(core, |st| st.screen_recording = false);
    match rec.stop() {
        Ok(f) => {
            session.ended_at = Some(chrono::Local::now());
            let _ = core.library.save_session(&session);
            if let Some(file) = f.file {
                let _ = core.library.add_file(&session, "screen", &session.title, &file, events, vec![], None);
                if core.settings.read().notifications.manual_capture {
                    core.notify(&tr!("Screen recording saved",
                        zh: "屏幕录制已保存", tw: "螢幕錄製已儲存",
                        es: "Grabación de pantalla guardada", fr: "Enregistrement d'écran enregistré",
                        ja: "画面録画を保存しました", ko: "화면 녹화 저장됨", ru: "Запись экрана сохранена"), &crate::i18n::title(&session.title));
                }
            }
        }
        Err(e) => log::error!("screen recording failed: {e:#}"),
    }
    core.library_changed();
}
