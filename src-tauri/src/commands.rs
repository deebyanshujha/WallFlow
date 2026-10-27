use crate::engine::{self, AppState};
use crate::models::{AppStatus, Settings, Wallpaper};
use crate::{db, scheduler, secrets, storage};
use std::sync::Arc;
use tauri::{AppHandle, State};

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_status(state: State<'_, Arc<AppState>>) -> CmdResult<AppStatus> {
    state.status().map_err(err)
}

#[tauri::command]
pub fn get_history(state: State<'_, Arc<AppState>>) -> CmdResult<Vec<Wallpaper>> {
    let conn = state.conn.lock().unwrap();
    db::list_wallpapers(&conn).map_err(err)
}

#[tauri::command]
pub async fn change_now(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> CmdResult<AppStatus> {
    let st = state.inner().clone();
    // A manual press is the one case where we always want to try the network.
    st.runtime.lock().unwrap().online_attempt_failed_at = None;
    scheduler::run_due_change(&app, &st, true).await;
    st.status().map_err(err)
}

#[tauri::command]
pub fn set_wallpaper_by_id(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> CmdResult<AppStatus> {
    let wallpaper = {
        let conn = state.conn.lock().unwrap();
        db::get_wallpaper(&conn, &id).map_err(err)?
    }
    .ok_or_else(|| "wallpaper not found".to_string())?;
    engine::apply(state.inner(), &wallpaper).map_err(err)?;
    state.status().map_err(err)
}

#[tauri::command]
pub fn toggle_favorite(
    state: State<'_, Arc<AppState>>,
    id: String,
    favorite: bool,
) -> CmdResult<()> {
    let conn = state.conn.lock().unwrap();
    db::set_favorite(&conn, &id, favorite).map_err(err)
}

#[tauri::command]
pub fn delete_wallpaper(state: State<'_, Arc<AppState>>, id: String) -> CmdResult<()> {
    let conn = state.conn.lock().unwrap();
    let settings = db::load_settings(&conn).map_err(err)?;
    if settings.current_wallpaper_id.as_deref() == Some(id.as_str()) {
        return Err("cannot delete the wallpaper that is currently in use".into());
    }
    if let Some(w) = db::get_wallpaper(&conn, &id).map_err(err)? {
        let path = state.paths.wallpaper_path(&w.file_name).map_err(err)?;
        let _ = std::fs::remove_file(path);
    }
    db::delete_wallpaper(&conn, &id).map_err(err)
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: Settings,
) -> CmdResult<AppStatus> {
    let mut incoming = settings;
    incoming.keep_recent = incoming.keep_recent.clamp(1, 200);
    incoming.max_storage_mb = incoming.max_storage_mb.clamp(50, 20_000);
    incoming.interval_hours = incoming.interval_hours.clamp(1, 720);
    incoming.preferred_hour = incoming.preferred_hour.min(23);
    incoming.search_query = incoming.search_query.trim().chars().take(100).collect();
    incoming.normalize();

    {
        let conn = state.conn.lock().unwrap();
        let previous = db::load_settings(&conn).map_err(err)?;
        // The frontend never owns these; keep the values the backend maintains.
        incoming.current_wallpaper_id = previous.current_wallpaper_id.clone();

        if previous.start_with_windows != incoming.start_with_windows {
            crate::set_autostart(&app, incoming.start_with_windows).map_err(err)?;
        }
        db::save_settings(&conn, &incoming).map_err(err)?;
    }

    crate::refresh_tray(&app);

    engine::schedule_next(state.inner(), &incoming).map_err(err)?;
    engine::enforce_storage_limits(state.inner()).map_err(err)?;
    state.status().map_err(err)
}

/// Copies the chosen image into our own directory so the fallback keeps working
/// even if the user later moves or deletes the original.
#[tauri::command]
pub fn set_default_wallpaper(
    state: State<'_, Arc<AppState>>,
    source_path: String,
) -> CmdResult<AppStatus> {
    let source = std::path::PathBuf::from(&source_path);
    if !source.is_file() {
        return Err("that file does not exist".into());
    }
    let bytes = std::fs::read(&source).map_err(err)?;
    // The user picked this file deliberately, so only reject something that is
    // not a usable image at all rather than holding it to the display size.
    let image = storage::validate_and_normalize(&bytes, 320, 240).map_err(err)?;

    let dest = state.paths.default_dir.join("default.jpg");
    storage::write_atomic(&dest, &image.jpeg).map_err(err)?;

    let conn = state.conn.lock().unwrap();
    let mut settings = db::load_settings(&conn).map_err(err)?;
    settings.default_wallpaper_path = Some(dest.to_string_lossy().to_string());
    db::save_settings(&conn, &settings).map_err(err)?;
    drop(conn);
    state.status().map_err(err)
}

#[tauri::command]
pub fn use_default_wallpaper(state: State<'_, Arc<AppState>>) -> CmdResult<AppStatus> {
    let settings = {
        let conn = state.conn.lock().unwrap();
        db::load_settings(&conn).map_err(err)?
    };
    engine::apply_default(state.inner(), &settings).map_err(err)?;
    state.status().map_err(err)
}

/// Takes the key, hands back nothing. The key is never readable from the UI again.
#[tauri::command]
pub fn set_api_key(
    state: State<'_, Arc<AppState>>,
    provider: String,
    api_key: String,
) -> CmdResult<AppStatus> {
    secrets::set_api_key(&provider, &api_key).map_err(err)?;
    state.status().map_err(err)
}

#[tauri::command]
pub fn open_panel(app: AppHandle) -> CmdResult<()> {
    crate::show_panel(&app);
    Ok(())
}
