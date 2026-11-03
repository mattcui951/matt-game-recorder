// First, so `tr!` is in scope in every module below.
#[macro_use]
pub mod i18n;
pub mod capture;
mod commands;
pub mod core;
mod discord;
pub mod editor;
pub mod ffmpeg;
pub mod import;
mod game;
pub mod input;
mod jobs;
pub mod ledger;
pub mod library;
pub mod lol;
mod media_server;
pub mod paths;
mod results;
pub mod update;
pub mod settings;
pub mod win;

use std::io::Write;
use tauri::menu::{Menu, MenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, RunEvent};
use tauri_plugin_global_shortcut::ShortcutState;

/// Minimal file logger: %APPDATA%/Matt Game Recorder/app.log, rotated at 5 MB.
struct FileLogger {
    file: parking_lot::Mutex<Option<std::fs::File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info || (m.level() <= log::Level::Debug && m.target().starts_with("mgr_lib"))
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) || record.level() == log::Level::Debug {
            return;
        }
        if let Some(f) = self.file.lock().as_mut() {
            let _ = writeln!(f, "{} {:5} {}: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

fn init_logging() {
    let path = paths::config_dir().join("app.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 5 * 1024 * 1024) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
    let logger = Box::leak(Box::new(FileLogger { file: parking_lot::Mutex::new(file) }));
    let _ = log::set_logger(logger);
    log::set_max_level(log::LevelFilter::Info);
}

/// `--import-outplayed <folder>` / `--import-library <folder>` start an import (also when passed
/// to a second launch, which forwards its arguments to the running app). Returns true if handled.
fn handle_cli(core: &std::sync::Arc<core::Core>, args: &[String]) -> bool {
    let pos = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    if let Some(folder) = pos("--import-outplayed") {
        let opts = import::Options { include_deleted: args.iter().any(|a| a == "--include-deleted"), delete_sources: false };
        import::start(core, "outplayed", folder.into(), false, opts);
        return true;
    }
    if let Some(folder) = pos("--import-library") {
        import::start(core, "library", folder.into(), false, Default::default());
        return true;
    }
    false
}

pub fn run() {
    init_logging();
    // After an update the installer restarts us with the old arguments; show the window anyway.
    let just_updated = update::take_marker();
    {
        let s = settings::Settings::load();
        log::info!("settings: language {} ({}), theme {}", i18n::resolve(&s.general.language), s.general.language, s.appearance.theme);
    }
    let started_minimized = std::env::args().any(|a| a == "--minimized") && !just_updated;

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(core) = core::get() {
                if !handle_cli(core, &args) {
                    core.show_main();
                }
            } else if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
            }
        }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|_app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Some(core) = core::get() {
                            core.on_shortcut(shortcut);
                        }
                    }
                })
                .build(),
        )
        .setup(move |app| {
            if let Ok(dir) = app.path().resource_dir() {
                paths::set_resource_dir(dir);
            }
            let core = core::Core::init(app.handle().clone());
            app.manage(core.clone());
            match media_server::start() {
                Ok(url) => {
                    log::info!("media server at {url}");
                    #[cfg(debug_assertions)]
                    let _ = std::fs::write(paths::config_dir().join("dev-media-url.txt"), &url);
                    *core.media_url.write() = url;
                }
                Err(e) => log::error!("media server: {e:#}"),
            }

            // Keep the Windows startup entry pointing at this exe (it moves when the app is
            // reinstalled or updated). Dev builds never touch it.
            #[cfg(not(debug_assertions))]
            {
                use tauri_plugin_autostart::ManagerExt;
                let al = app.autolaunch();
                let result = if core.settings().general.start_with_windows { al.enable() } else { al.disable() };
                if let Err(e) = result {
                    log::warn!("could not update start-with-Windows entry: {e}");
                }
            }
            if !settings::Settings::path().exists() {
                let _ = core.settings().save();
            }

            let menu = tray_menu(app.handle())?;
            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().cloned().expect("icon"))
                .tooltip("Matt Game Recorder")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    let Some(core) = core::get() else { return };
                    match event.id().as_ref() {
                        "open" => core.show_main(),
                        "screen" => core.send(game::Cmd::ToggleScreenRecorder),
                        "quit" => {
                            core.quit();
                            let _ = app;
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|_tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        if let Some(core) = core::get() {
                            core.show_main();
                        }
                    }
                })
                .build(app)?;

            core.register_hotkeys(false);
            {
                let core = core.clone();
                std::thread::spawn(move || {
                    // Recover crash leftovers before any new recording can start.
                    if core.library.recover_partials() > 0 {
                        core.library_changed();
                    }
                    let merged = core.library.merge_all_multikills();
                    if merged > 0 {
                        log::info!("merged multikill streaks in {merged} recordings");
                        core.library_changed();
                    }
                    let renamed = core.library.renumber_all();
                    if renamed > 0 {
                        log::info!("renamed {renamed} recordings to match their match numbers");
                        core.library_changed();
                    }
                    let fixed = core.library.repair_imported_results();
                    if fixed > 0 {
                        log::info!("corrected the result of {fixed} imported matches");
                        core.library_changed();
                    }
                    game::spawn(core.clone());
                    results::spawn(core.clone());
                    update::spawn(core.clone());
                    let args: Vec<String> = std::env::args().collect();
                    if !handle_cli(&core, &args) {
                        // Continue an import that a restart or crash interrupted.
                        if let Some(p) = import::pending() {
                            log::info!("resuming {} import from {}{}", p.kind, p.folder.display(), if p.paused { " (paused)" } else { "" });
                            import::start(&core, &p.kind, p.folder, p.paused, p.options);
                        }
                    }
                    // Warm the encoder probe so Settings opens instantly, then tidy storage.
                    let _ = ffmpeg::encoders();
                    core.housekeeping();
                });
            }
            if !started_minimized {
                core.show_main();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::save_settings,
            commands::list_devices,
            commands::get_library,
            commands::get_status,
            commands::set_favorite,
            commands::rename_recording,
            commands::rename_session,
            commands::delete_recordings,
            commands::restore_recordings,
            commands::purge_recordings,
            commands::reveal_recording,
            commands::open_media_folder,
            commands::disk_info,
            commands::quick_trim,
            commands::supercut,
            commands::cancel_job,
            commands::get_sprite,
            commands::toggle_screen_recorder,
            commands::ensure_art,
            commands::champion_names_zh,
            commands::check_update,
            commands::install_update,
            commands::list_projects,
            commands::get_project,
            commands::create_project,
            commands::save_project,
            commands::delete_project,
            commands::export_project,
            commands::change_media_folder,
            commands::create_backup,
            commands::empty_recently_deleted,
            commands::quit_app,
            commands::scan_outplayed,
            commands::import_outplayed,
            commands::import_library,
            commands::list_jobs,
            commands::pause_job,
            commands::import_cpu_assist,
            commands::system_fonts,
            commands::read_theme_file,
            commands::write_theme_file,
            commands::set_import_cpu_assist,
            commands::delete_sessions,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Matt Game Recorder");

    app.run(|_app, event| {
        // Closing every window keeps the recorder running in the tray; only Quit exits.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            let quitting = core::get().is_some_and(|c| c.is_quitting());
            if code.is_none() && !quitting {
                api.prevent_exit();
            }
        }
    });
}

/// The tray icon's menu, in the current language.
pub fn tray_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    MenuBuilder::new(app)
        .text("open", tr!("Open Matt Game Recorder", "打开 Matt Game Recorder"))
        .text("screen", tr!("Start / stop screen recording", "开始 / 停止屏幕录制"))
        .separator()
        .text("quit", tr!("Quit", "退出"))
        .build()
}
