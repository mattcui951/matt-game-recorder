//! Shared application state and the services every module uses: settings, library, windows,
//! notifications, global hotkeys and storage housekeeping.

use crate::game::Cmd;
use crate::jobs::Jobs;
use crate::library::Library;
use crate::settings::Settings;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use tauri::webview::Color;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use tauri_plugin_notification::NotificationExt;

static CORE: OnceLock<Arc<Core>> = OnceLock::new();

/// Debug builds: `MGR_DEVTOOLS_PORT=9333` opens the webviews to DevTools-protocol test scripts.
/// Every window must use the same arguments, since they share one WebView2 environment.
/// Debug builds get a WebView data folder of their own, so a test run never changes what the
/// installed app's windows remember; and the DevTools port when `MGR_DEVTOOLS_PORT` is set.
fn with_debug_port<'a>(b: WebviewWindowBuilder<'a, tauri::Wry, AppHandle>) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    #[cfg(debug_assertions)]
    {
        let b = b.data_directory(crate::paths::config_dir().join("webview-dev"));
        if let Ok(port) = std::env::var("MGR_DEVTOOLS_PORT") {
            // Keep wry's defaults; setting arguments replaces them.
            return b.additional_browser_args(&format!(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"
            ));
        }
        b
    }
    #[cfg(not(debug_assertions))]
    b
}

pub fn get() -> Option<&'static Arc<Core>> {
    CORE.get()
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// idle | waiting | recording | saving | error
    pub state: String,
    /// match | highlights | manual | session
    pub mode: Option<String>,
    pub session_id: Option<String>,
    pub title: Option<String>,
    pub elapsed: f64,
    pub champion: Option<String>,
    pub champion_key: Option<String>,
    pub kda: Option<crate::lol::Kda>,
    /// good | partial | down | idle
    pub events_status: String,
    pub events: Vec<crate::library::RecEvent>,
    pub clips_saved: u32,
    pub bookmarking: bool,
    pub screen_recording: bool,
    pub screen_elapsed: f64,
    pub message: Option<String>,
    pub apm: u32,
}

pub struct Core {
    pub app: AppHandle,
    pub settings: RwLock<Settings>,
    pub library: Library,
    pub jobs: Jobs,
    pub status: RwLock<Status>,
    pub media_url: RwLock<String>,
    pub cmd: Mutex<Option<Sender<Cmd>>>,
    in_game_hotkeys: AtomicBool,
    quitting: AtomicBool,
}

/// Hotkey actions. Several can share one key; the game loop decides by context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hotkey {
    BookmarkToggle,
    PlaceBookmark,
    Screenshot,
    ToggleApp,
    ToggleWebcam,
    ToggleDashboard,
    DashboardLayout,
    ScreenToggle,
    ScreenBookmark,
}

impl Core {
    pub fn init(app: AppHandle) -> Arc<Core> {
        let settings = Settings::load();
        crate::i18n::set(&settings.general.language);
        let library = Library::new(settings.media_folder());
        let core = Arc::new(Core {
            app,
            settings: RwLock::new(settings),
            library,
            jobs: Jobs::default(),
            status: RwLock::new(Status { state: "idle".into(), events_status: "idle".into(), ..Default::default() }),
            media_url: RwLock::new(String::new()),
            cmd: Mutex::new(None),
            in_game_hotkeys: AtomicBool::new(false),
            quitting: AtomicBool::new(false),
        });
        let _ = CORE.set(core.clone());
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    pub fn send(&self, cmd: Cmd) {
        if let Some(tx) = self.cmd.lock().as_ref() {
            let _ = tx.send(cmd);
        }
    }

    pub fn emit_status(&self) {
        let s = self.status.read().clone();
        let _ = self.app.emit("status", s);
    }

    pub fn library_changed(&self) {
        let _ = self.app.emit("library-changed", ());
    }

    pub fn notify(&self, title: &str, body: &str) {
        let _ = self.app.notification().builder().title(title).body(body).show();
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::Relaxed)
    }

    pub fn quit(&self) {
        self.quitting.store(true, Ordering::Relaxed);
        self.send(Cmd::Shutdown);
    }

    // ---------- windows ----------

    /// Show the main window, creating it if it was closed (closing frees the webview's memory).
    pub fn show_main(&self) {
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            return;
        }
        let built = WebviewWindowBuilder::new(&self.app, "main", WebviewUrl::App("index.html".into()))
            .title("Matt Game Recorder")
            .inner_size(1440.0, 900.0)
            .min_inner_size(1100.0, 680.0)
            .decorations(false)
            .background_color(self.window_color())
            .center();
        let built = with_debug_port(built).build();
        match built {
            Ok(w) => self.apply_capture_affinity(&w),
            Err(e) => log::error!("cannot open main window: {e}"),
        }
    }

    /// The theme's background, painted before the page loads so windows never flash another color.
    fn window_color(&self) -> Color {
        let (r, g, b) = self.settings().appearance.window_rgb();
        Color(r, g, b, 255)
    }

    pub fn toggle_main_on_top(&self) {
        match self.app.get_webview_window("main") {
            Some(w) if w.is_visible().unwrap_or(false) && w.is_always_on_top().unwrap_or(false) => {
                let _ = w.set_always_on_top(false);
                let _ = w.minimize();
            }
            _ => {
                self.show_main();
                if let Some(w) = self.app.get_webview_window("main") {
                    let _ = w.set_always_on_top(true);
                }
            }
        }
    }

    /// Our windows are hidden from capture only while a match is recorded (per the Capture
    /// settings). Outside games they stay visible to screen recordings and screenshots.
    fn apply_capture_affinity(&self, w: &tauri::WebviewWindow) {
        let in_game = self.in_game_hotkeys.load(Ordering::Relaxed);
        let show = !in_game
            || match w.label() {
                "main" => self.settings.read().general.capture_own_windows,
                _ => self.settings.read().general.capture_notifications,
            };
        if let Ok(h) = w.hwnd() {
            crate::win::exclude_from_capture(h.0 as isize, !show);
        }
    }

    pub fn refresh_capture_affinity(&self) {
        // Window display affinity is changed on the UI thread.
        let _ = self.app.run_on_main_thread(|| {
            if let Some(core) = get() {
                for (_, w) in core.app.webview_windows() {
                    core.apply_capture_affinity(&w);
                }
            }
        });
    }

    /// The in-game dashboard on a monitor other than the game's.
    pub fn open_dashboard(&self, game_hwnd: isize) {
        if self.app.get_webview_window("dashboard").is_some() {
            return;
        }
        let game_mon = crate::win::monitor_of_window(game_hwnd);
        let Some(mon) = crate::win::monitors().into_iter().find(|m| m.handle != game_mon) else { return };
        let scale = 1.0_f64.max(self.app.primary_monitor().ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0));
        let (w, h) = (400.0, 640.0);
        let x = mon.x as f64 / scale + mon.width as f64 / scale - w - 24.0;
        let y = mon.y as f64 / scale + 24.0;
        let built = WebviewWindowBuilder::new(&self.app, "dashboard", WebviewUrl::App("index.html#dashboard".into()))
            .title("Matt Game Recorder - Live")
            .inner_size(w, h)
            .position(x, y)
            .decorations(false)
            .resizable(true)
            .background_color(self.window_color());
        let built = with_debug_port(built).build();
        if let Ok(win) = built {
            self.apply_capture_affinity(&win);
        }
    }

    pub fn close_dashboard(&self) {
        if let Some(w) = self.app.get_webview_window("dashboard") {
            let _ = w.close();
        }
    }

    pub fn toggle_dashboard(&self, game_hwnd: isize) {
        if self.app.get_webview_window("dashboard").is_some() {
            self.close_dashboard();
        } else {
            self.open_dashboard(game_hwnd);
        }
    }

    // ---------- hotkeys ----------

    /// (Re)register global hotkeys. In-game keys (bookmarks, screenshot, Ctrl+O...) are only
    /// grabbed while a match is being recorded so they never steal shortcuts from other apps.
    pub fn register_hotkeys(&self, in_game: bool) {
        self.in_game_hotkeys.store(in_game, Ordering::Relaxed);
        // Entering or leaving a match changes whether our windows may appear in captures.
        self.refresh_capture_affinity();
        let gs = self.app.global_shortcut();
        let _ = gs.unregister_all();
        // One key can serve several actions; register each distinct key once.
        let mut keys: Vec<Shortcut> = Vec::new();
        for (key, _) in self.hotkey_table(in_game) {
            match key.parse::<Shortcut>() {
                Ok(sc) if !keys.contains(&sc) => keys.push(sc),
                Ok(_) => {}
                Err(e) => log::warn!("hotkey {key} is not valid: {e}"),
            }
        }
        for sc in keys {
            if let Err(e) = gs.register(sc) {
                log::warn!("hotkey {sc:?} unavailable (another app may own it): {e}");
            }
        }
    }

    pub fn reregister_hotkeys(&self) {
        self.register_hotkeys(self.in_game_hotkeys.load(Ordering::Relaxed));
    }

    fn hotkey_table(&self, in_game: bool) -> Vec<(String, Hotkey)> {
        let s = self.settings.read();
        let sr = &s.screen_recorder;
        let mut v = vec![(sr.hotkey_toggle.clone(), Hotkey::ScreenToggle), (sr.hotkey_bookmark.clone(), Hotkey::ScreenBookmark)];
        if in_game {
            let h = &s.games.lol.hotkeys;
            v.extend([
                (h.bookmark_toggle.clone(), Hotkey::BookmarkToggle),
                (h.place_bookmark.clone(), Hotkey::PlaceBookmark),
                (h.screenshot.clone(), Hotkey::Screenshot),
                (h.toggle_app.clone(), Hotkey::ToggleApp),
                (h.toggle_webcam.clone(), Hotkey::ToggleWebcam),
                (h.toggle_dashboard.clone(), Hotkey::ToggleDashboard),
                (h.dashboard_layout.clone(), Hotkey::DashboardLayout),
            ]);
        }
        v.retain(|(k, _)| !k.trim().is_empty());
        v
    }

    pub fn on_shortcut(&self, shortcut: &Shortcut) {
        let in_game = self.in_game_hotkeys.load(Ordering::Relaxed);
        let actions: Vec<Hotkey> = self
            .hotkey_table(in_game)
            .into_iter()
            .filter(|(k, _)| k.parse::<Shortcut>().ok().as_ref() == Some(shortcut))
            .map(|(_, a)| a)
            .collect();
        // A key bound to both a game action and a screen-recorder action means the game action in game.
        let pick = |game: Hotkey, screen: Hotkey| if in_game && actions.contains(&game) { Some(game) } else if actions.contains(&screen) { Some(screen) } else { None };
        let chosen = pick(Hotkey::BookmarkToggle, Hotkey::ScreenToggle)
            .or_else(|| pick(Hotkey::PlaceBookmark, Hotkey::ScreenBookmark))
            .or_else(|| actions.first().copied());
        if let Some(a) = chosen {
            self.send(Cmd::Hotkey(a));
        }
    }

    // ---------- storage ----------

    /// Apply the retention rules: age and size limits move recordings to Recently deleted,
    /// and Recently deleted is emptied of anything older than its retention period.
    pub fn housekeeping(&self) {
        let s = self.settings();
        let st = &s.storage;
        let protected = crate::editor::protected_ids(&self.library.root());
        let mut removed = 0;
        if st.age_limit_enabled && st.age_limit_months > 0 {
            removed += self.library.enforce_age(st.age_limit_months.saturating_mul(30), &protected);
        }
        if st.mode == "auto" {
            removed += self.library.enforce_limit(st.limit_gb as u64 * 1024 * 1024 * 1024, &protected);
        }
        let purged = self.library.purge_expired(st.keep_deleted_days);
        if removed + purged > 0 {
            log::info!("retention: {removed} recordings moved to Recently deleted, {purged} removed for good");
            self.library_changed();
        }
    }
}
