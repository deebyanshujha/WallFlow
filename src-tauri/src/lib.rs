mod commands;
mod db;
mod engine;
mod models;
mod platform;
mod providers;
mod scheduler;
mod secrets;
mod storage;

use crate::engine::{AppState, Runtime};
use anyhow::Result;
use chrono::{Local, TimeZone};
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

/// The two tray entries whose labels track application state.
struct TrayItems {
    current: MenuItem<tauri::Wry>,
    next: MenuItem<tauri::Wry>,
}

pub fn show_panel(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<()> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable()?;
    } else {
        manager.disable()?;
    }
    Ok(())
}

/// Pushes the current wallpaper and next-change time into the tray menu.
pub fn refresh_tray(app: &AppHandle) {
    let (Some(items), Some(state)) = (
        app.try_state::<TrayItems>(),
        app.try_state::<Arc<AppState>>(),
    ) else {
        return;
    };
    let Ok(status) = state.status() else {
        return;
    };

    let current = match &status.current {
        Some(w) => match &w.author_name {
            Some(author) => format!("Current: {} — {}", w.category, author),
            None => format!("Current: {}", w.category),
        },
        None => "Current: default wallpaper".to_string(),
    };
    let _ = items.current.set_text(current);

    let next = match status.settings.next_change_at {
        0 => "Next: not scheduled".to_string(),
        ts => match Local.timestamp_opt(ts, 0).single() {
            Some(t) => format!("Next: {}", t.format("%a %d %b, %H:%M")),
            None => "Next: not scheduled".to_string(),
        },
    };
    let _ = items.next.set_text(next);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin registered so a second launch is rejected
        // before it does any other startup work.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_panel(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_history,
            commands::change_now,
            commands::set_wallpaper_by_id,
            commands::toggle_favorite,
            commands::delete_wallpaper,
            commands::save_settings,
            commands::set_default_wallpaper,
            commands::use_default_wallpaper,
            commands::set_api_key,
            commands::open_panel,
        ])
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            let paths = storage::Paths::new(data_dir)?;
            let conn = db::open(&paths.db)?;

            let mut bundled_default = app
                .path()
                .resolve(
                    "resources/default-wallpaper.png",
                    tauri::path::BaseDirectory::Resource,
                )
                .unwrap_or_default();
            if cfg!(debug_assertions) && !bundled_default.is_file() {
                // `tauri dev` runs from the crate root without a bundled resource dir.
                bundled_default = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("resources/default-wallpaper.png");
            }

            let client = reqwest::Client::builder()
                .user_agent(concat!("WallFlow/", env!("CARGO_PKG_VERSION")))
                .https_only(true)
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()?;

            let state = Arc::new(AppState {
                paths,
                conn: Mutex::new(conn),
                client,
                runtime: Mutex::new(Runtime::default()),
                bundled_default,
            });
            app.manage(state.clone());

            // Keep the OS registration in step with the saved preference, in case
            // it was changed outside the app or the install was moved.
            let wants_autostart = {
                let conn = state.conn.lock().unwrap();
                db::load_settings(&conn).map(|s| s.start_with_windows).unwrap_or(false)
            };
            let handle = app.handle();
            if handle.autolaunch().is_enabled().unwrap_or(false) != wants_autostart {
                let _ = set_autostart(handle, wants_autostart);
            }

            build_tray(handle)?;
            refresh_tray(handle);

            // Launched by the autostart entry: stay in the tray, no window.
            let minimized = std::env::args().any(|a| a == "--minimized");
            if !minimized {
                show_panel(handle);
            }

            scheduler::spawn(handle.clone(), state);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the panel must not stop the scheduler, so hide instead of
            // exit. Only "Exit WallFlow" in the tray really quits.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running WallFlow");
}

fn change_wallpaper_from_tray(app: &AppHandle) {
    let handle = app.clone();
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        let state = state.inner().clone();
        tauri::async_runtime::spawn(async move {
            state.runtime.lock().unwrap().online_attempt_failed_at = None;
            scheduler::run_due_change(&handle, &state, true).await;
            refresh_tray(&handle);
        });
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open WallFlow", true, None::<&str>)?;
    let change = MenuItem::with_id(app, "change", "Change wallpaper now", true, None::<&str>)?;
    // Informational rows, so they are present but not clickable.
    let current = MenuItem::with_id(app, "current", "Current: —", false, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Next: —", false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit WallFlow", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &open, &change, &sep1, &current, &next, &sep2, &settings, &exit,
        ],
    )?;

    app.manage(TrayItems { current, next });

    TrayIconBuilder::with_id("wallflow-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("WallFlow")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_panel(app),
            "change" => change_wallpaper_from_tray(app),
            "settings" => {
                show_panel(app);
                let _ = app.emit("wallflow://navigate", "settings");
            }
            "exit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_panel(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
