//! User settings, persisted as JSON in the app config dir.
//! Every struct uses `#[serde(default)]` so older files keep loading after new fields are added.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Event kinds that can be auto-bookmarked. Keys are stable strings shared with the UI.
pub const EVENT_KINDS: &[&str] = &[
    "victory", "kill", "death", "assist", "ultimate", "baron", "dragon", "herald", "turret", "inhibitor",
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Timing {
    pub before: f64,
    pub after: f64,
}

pub fn default_timing(kind: &str) -> Timing {
    let (before, after) = match kind {
        "victory" => (10.0, 1.0),
        "kill" | "assist" => (12.0, 8.0),
        "death" => (12.0, 3.0),
        "ultimate" => (5.0, 5.0),
        "bookmark" => (20.0, 0.0),
        _ => (10.0, 5.0),
    };
    Timing { before, after }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct General {
    /// Show the recorder's own windows in captures (false = exclude them via display affinity).
    pub capture_own_windows: bool,
    pub capture_notifications: bool,
    pub discord_presence: bool,
    pub discord_app_id: String,
    pub watermark: bool,
    pub start_with_windows: bool,
    /// "art" shows champion art behind the UI, "plain" hides it.
    pub layout: String,
    /// system | en | zh-CN
    pub language: String,
    /// Look for new versions on GitHub in the background.
    pub check_updates: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            capture_own_windows: false,
            capture_notifications: false,
            discord_presence: false,
            discord_app_id: String::new(),
            watermark: false,
            start_with_windows: true,
            layout: "art".into(),
            language: "system".into(),
            check_updates: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Video {
    /// low | medium | high | custom
    pub preset: String,
    /// Output height in pixels; 0 means the game's native size.
    pub resolution: u32,
    pub bitrate_kbps: u32,
    pub fps: u32,
    pub force_16_9: bool,
    /// fit | stretch (only used with force_16_9)
    pub crop_mode: String,
    /// ffmpeg encoder name, e.g. av1_nvenc
    pub encoder: String,
    /// default | quality | performance | lowlatency | perf_lowlatency | quality_lowlatency
    pub encoder_preset: String,
    /// ram | disk
    pub buffer: String,
}

impl Default for Video {
    fn default() -> Self {
        Self {
            preset: "high".into(),
            resolution: 1080,
            bitrate_kbps: 8000,
            fps: 60,
            force_16_9: false,
            crop_mode: "fit".into(),
            encoder: "av1_nvenc".into(),
            encoder_preset: "default".into(),
            buffer: "ram".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Audio {
    pub system_enabled: bool,
    /// WASAPI endpoint id, or "default"
    pub system_device: String,
    pub system_volume: u32,
    pub mic_enabled: bool,
    pub mic_device: String,
    pub mic_volume: u32,
    /// Capture only the game process audio instead of the whole output device.
    pub game_only: bool,
    pub mic_mono: bool,
}

impl Default for Audio {
    fn default() -> Self {
        Self {
            system_enabled: true,
            system_device: "default".into(),
            system_volume: 100,
            mic_enabled: true,
            mic_device: "default".into(),
            mic_volume: 100,
            game_only: false,
            mic_mono: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Webcam {
    pub enabled: bool,
    pub device: String,
    /// always | visible
    pub mode: String,
}

impl Default for Webcam {
    fn default() -> Self {
        Self { enabled: false, device: String::new(), mode: "always".into() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ScreenRecorder {
    pub capture_cursor: bool,
    pub show_border: bool,
    /// Index into the monitor list.
    pub display: u32,
    pub hotkey_toggle: String,
    pub hotkey_bookmark: String,
    pub bookmark_before: f64,
    pub bookmark_after: f64,
}

impl Default for ScreenRecorder {
    fn default() -> Self {
        Self {
            capture_cursor: true,
            show_border: true,
            display: 0,
            hotkey_toggle: "Ctrl+F4".into(),
            hotkey_bookmark: "F8".into(),
            bookmark_before: 20.0,
            bookmark_after: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Storage {
    /// "auto" keeps the library under `limit_gb`; "manual" has no size limit.
    pub mode: String,
    pub limit_gb: u32,
    /// Remove recordings of matches older than `age_limit_months`.
    pub age_limit_enabled: bool,
    pub age_limit_months: u32,
    /// Days a deleted recording stays in Recently deleted before it is removed for good.
    pub keep_deleted_days: u32,
    pub media_folder: String,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            mode: "manual".into(),
            limit_gb: 200,
            age_limit_enabled: false,
            age_limit_months: 6,
            keep_deleted_days: 3,
            media_folder: default_media_folder().to_string_lossy().into_owned(),
        }
    }
}

/// New installs record into Videos; Settings > Storage moves the library anywhere.
fn default_media_folder() -> PathBuf {
    dirs::video_dir().unwrap_or_else(|| PathBuf::from(r"C:\")).join("Matt Game Recorder")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Notifications {
    pub session_start: bool,
    pub match_start: bool,
    pub match_end: bool,
    pub manual_capture: bool,
    pub game_event_capture: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { session_start: true, match_start: true, match_end: true, manual_capture: true, game_event_capture: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GameHotkeys {
    pub bookmark_toggle: String,
    pub bookmark_toggle_before: f64,
    pub place_bookmark: String,
    pub place_before: f64,
    pub place_after: f64,
    pub screenshot: String,
    pub toggle_app: String,
    pub toggle_webcam: String,
    pub toggle_dashboard: String,
    pub dashboard_layout: String,
    /// Key used to cast the ultimate in game, for the Ultimate event.
    pub ultimate_key: String,
}

impl Default for GameHotkeys {
    fn default() -> Self {
        Self {
            bookmark_toggle: "Ctrl+F4".into(),
            bookmark_toggle_before: 5.0,
            place_bookmark: "F8".into(),
            place_before: 20.0,
            place_after: 0.0,
            screenshot: "Ctrl+F2".into(),
            toggle_app: "Ctrl+O".into(),
            toggle_webcam: "Ctrl+F10".into(),
            toggle_dashboard: "Ctrl+F11".into(),
            dashboard_layout: "Ctrl+L".into(),
            ultimate_key: "R".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GameSettings {
    pub enabled: bool,
    pub open_app_on_end: bool,
    pub dashboard_on_secondary: bool,
    pub collect_apm: bool,
    pub capture_cursor: bool,
    /// manual | highlights | match | session
    pub capture_mode: String,
    pub events: BTreeMap<String, bool>,
    pub timing: BTreeMap<String, Timing>,
    pub hotkeys: GameHotkeys,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            open_app_on_end: true,
            dashboard_on_secondary: false,
            collect_apm: true,
            capture_cursor: true,
            capture_mode: "match".into(),
            events: EVENT_KINDS.iter().map(|k| (k.to_string(), true)).collect(),
            timing: EVENT_KINDS.iter().map(|k| (k.to_string(), default_timing(k))).collect(),
            hotkeys: GameHotkeys::default(),
        }
    }
}

impl GameSettings {
    pub fn event_enabled(&self, kind: &str) -> bool {
        match kind {
            "bookmark" => true,
            // Multikills ride on the kill toggle.
            "multikill" => self.events.get("kill").copied().unwrap_or(true),
            k => self.events.get(k).copied().unwrap_or(true),
        }
    }

    pub fn timing_for(&self, kind: &str) -> Timing {
        let key = if kind == "multikill" { "kill" } else { kind };
        self.timing.get(key).copied().unwrap_or_else(|| default_timing(key))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Games {
    pub lol: GameSettings,
}

/// Theme and look. Colors are `#rrggbb` keyed by UI token (bg, panel, text, you, ...); the
/// theme list and the token set live in the UI (src/lib/themes.ts).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Appearance {
    pub theme: String,
    /// Follow Windows light/dark mode, using `light_theme` and `dark_theme`.
    pub follow_system: bool,
    pub light_theme: String,
    pub dark_theme: String,
    /// Tweaks to built-in themes: theme id -> token -> color.
    pub overrides: BTreeMap<String, BTreeMap<String, String>>,
    /// Empty = the theme's own font.
    pub font_ui: String,
    pub font_score: String,
    /// Interface zoom, percent.
    pub ui_scale: u32,
    /// "sharp" | "default" | "round"
    pub corners: String,
    pub custom_themes: Vec<CustomTheme>,
    /// The active theme's background, so new windows open in it instead of flashing.
    pub window_bg: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "arcade".into(),
            follow_system: false,
            light_theme: "rose-pine-dawn".into(),
            dark_theme: "rose-pine-moon".into(),
            overrides: BTreeMap::new(),
            font_ui: String::new(),
            font_score: String::new(),
            ui_scale: 100,
            corners: "default".into(),
            custom_themes: Vec::new(),
            window_bg: "#100d26".into(),
        }
    }
}

impl Appearance {
    /// `window_bg` as RGB, falling back to the default theme's background.
    pub fn window_rgb(&self) -> (u8, u8, u8) {
        let hex = self.window_bg.trim_start_matches('#');
        match u32::from_str_radix(hex, 16) {
            Ok(n) if hex.len() == 6 => ((n >> 16) as u8, (n >> 8) as u8, n as u8),
            _ => (16, 13, 38),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub dark: bool,
    pub colors: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub general: General,
    pub video: Video,
    pub audio: Audio,
    pub webcam: Webcam,
    pub screen_recorder: ScreenRecorder,
    pub storage: Storage,
    pub notifications: Notifications,
    pub games: Games,
    pub appearance: Appearance,
}

impl Settings {
    pub fn path() -> PathBuf {
        crate::paths::config_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        match std::fs::read_to_string(Self::path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("settings.json unreadable ({e}); using defaults");
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn media_folder(&self) -> PathBuf {
        PathBuf::from(&self.storage.media_folder)
    }
}
