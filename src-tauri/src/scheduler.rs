use crate::engine::{self, AppState};
use crate::db;
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// A single low-frequency timer. It wakes once a minute, compares two integers,
/// and goes back to sleep — it does not touch the network unless a change is due.
pub fn spawn(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        // Launching is not by itself a reason to change the wallpaper. If the
        // schedule says nothing is due, re-apply what we already picked — that
        // costs no network, so starting with Windows never downloads anything.
        if is_change_due(&state) {
            run_due_change(&app, &state, false).await;
        } else {
            engine::reapply_current(&state);
            crate::refresh_tray(&app);
        }

        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            if is_change_due(&state) {
                run_due_change(&app, &state, false).await;
            }
        }
    });
}

/// A change is due once the scheduled moment has passed. A zero means the app
/// has never scheduled one yet (fresh install), which counts as due.
fn is_change_due(state: &AppState) -> bool {
    let conn = state.conn.lock().unwrap();
    match db::load_settings(&conn) {
        Ok(s) => s.next_change_at == 0 || Utc::now().timestamp() >= s.next_change_at,
        Err(_) => false,
    }
}

pub async fn run_due_change(app: &AppHandle, state: &AppState, force_online: bool) {
    if let Err(e) = engine::change_wallpaper(state, force_online).await {
        let mut rt = state.runtime.lock().unwrap();
        rt.last_error = Some(e.to_string());
        rt.network_active = false;
    }
    crate::refresh_tray(app);
    let _ = app.emit("wallflow://status-changed", ());
}
