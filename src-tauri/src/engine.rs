use crate::models::{AppStatus, FetchRequest, Settings, Wallpaper};
use crate::providers::Provider;
use crate::{db, platform, secrets, storage};
use anyhow::{anyhow, Result};
use chrono::{Duration, Local, TimeZone, Timelike, Utc};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub paths: storage::Paths,
    pub conn: Mutex<Connection>,
    pub client: reqwest::Client,
    pub runtime: Mutex<Runtime>,
    pub bundled_default: PathBuf,
}

#[derive(Default)]
pub struct Runtime {
    pub network_active: bool,
    pub last_message: Option<String>,
    pub last_error: Option<String>,
    /// Set after a failed online attempt so the scheduler does not retry in a
    /// loop — the next try happens at the next scheduled change, not sooner.
    pub online_attempt_failed_at: Option<i64>,
}

impl AppState {
    pub fn status(&self) -> Result<AppStatus> {
        let conn = self.conn.lock().unwrap();
        let settings = db::load_settings(&conn)?;
        let current = match &settings.current_wallpaper_id {
            Some(id) => db::get_wallpaper(&conn, id)?,
            None => None,
        };
        let (storage_used_bytes, wallpaper_count) = db::storage_stats(&conn)?;
        let rt = self.runtime.lock().unwrap();
        let (screen_width, screen_height) = platform::primary_resolution();
        Ok(AppStatus {
            has_api_key: secrets::has_api_key(&settings.provider),
            settings,
            current,
            network_active: rt.network_active,
            last_message: rt.last_message.clone(),
            last_error: rt.last_error.clone(),
            storage_used_bytes,
            wallpaper_count,
            screen_width,
            screen_height,
        })
    }

    fn note(&self, message: impl Into<String>) {
        let mut rt = self.runtime.lock().unwrap();
        rt.last_message = Some(message.into());
        rt.last_error = None;
    }

    fn note_error(&self, message: impl Into<String>) {
        let mut rt = self.runtime.lock().unwrap();
        rt.last_error = Some(message.into());
    }

    fn set_network_active(&self, active: bool) {
        self.runtime.lock().unwrap().network_active = active;
    }
}

/// The whole point of the app, in one function.
///
/// Priority is strictly: new download → local cache → default wallpaper.
/// Every branch ends with a wallpaper on screen; none of them can panic the app.
pub async fn change_wallpaper(state: &AppState, force_online: bool) -> Result<()> {
    let (settings, exclude) = {
        let conn = state.conn.lock().unwrap();
        let s = db::load_settings(&conn)?;
        let e = db::known_remote_ids(&conn, &s.provider, 100)?;
        (s, e)
    };

    let may_go_online = force_online || should_attempt_online(state, &settings);

    if may_go_online {
        match try_download(state, &settings, exclude).await {
            Ok(w) => {
                apply(state, &w)?;
                state.note(format!(
                    "Downloaded a new wallpaper from {}.",
                    settings.provider
                ));
                schedule_next(state, &settings)?;
                return Ok(());
            }
            Err(e) => {
                state.set_network_active(false);
                state.runtime.lock().unwrap().online_attempt_failed_at =
                    Some(Utc::now().timestamp());
                state.note_error(format!("{e}. Using a cached wallpaper instead."));
            }
        }
    }

    // Offline / failed path: reuse what we already have.
    let cached = {
        let conn = state.conn.lock().unwrap();
        db::pick_cached(
            &conn,
            &settings.category,
            settings.current_wallpaper_id.as_deref(),
        )?
    };

    if let Some(w) = cached {
        apply(state, &w)?;
        let had_error = state.runtime.lock().unwrap().last_error.is_some();
        if !had_error {
            state.note("Using a cached wallpaper. No download was needed.");
        }
        schedule_next(state, &settings)?;
        return Ok(());
    }

    apply_default(state, &settings)?;
    schedule_next(state, &settings)?;
    Ok(())
}

/// Puts the already-chosen wallpaper back on the desktop without touching the
/// network. Used at launch when no change is due, so the desktop is still right
/// after a reboot or after something else changed it.
pub fn reapply_current(state: &AppState) {
    let settings = {
        let conn = state.conn.lock().unwrap();
        match db::load_settings(&conn) {
            Ok(s) => s,
            Err(_) => return,
        }
    };

    let current = settings.current_wallpaper_id.as_ref().and_then(|id| {
        let conn = state.conn.lock().unwrap();
        db::get_wallpaper(&conn, id).ok().flatten()
    });

    match current {
        // If the cached file vanished, apply() prunes it and we fall back.
        Some(w) if apply(state, &w).is_ok() => {}
        _ => {
            let _ = apply_default(state, &settings);
        }
    }
}

/// Falls back to the user's chosen default, then to the wallpaper we ship, so
/// there is always something safe on screen on a first offline launch.
pub fn apply_default(state: &AppState, settings: &Settings) -> Result<()> {
    let user_default = settings
        .default_wallpaper_path
        .as_ref()
        .map(PathBuf::from)
        .filter(|p| p.is_file());

    let path = match user_default {
        Some(p) => p,
        None if state.bundled_default.is_file() => state.bundled_default.clone(),
        None => return Err(anyhow!("no cached, default, or bundled wallpaper available")),
    };

    platform::set_wallpaper(&path)?;
    {
        let conn = state.conn.lock().unwrap();
        let mut s = db::load_settings(&conn)?;
        s.current_wallpaper_id = None;
        db::save_settings(&conn, &s)?;
    }
    state.note("No cached wallpapers yet — using your default wallpaper.");
    Ok(())
}

pub fn apply(state: &AppState, w: &Wallpaper) -> Result<()> {
    let path = state.paths.wallpaper_path(&w.file_name)?;
    if !path.is_file() {
        // Metadata outlived the file (manual deletion, disk cleanup).
        let conn = state.conn.lock().unwrap();
        db::delete_wallpaper(&conn, &w.id)?;
        return Err(anyhow!("cached file was missing; removed it from history"));
    }
    platform::set_wallpaper(&path)?;

    let conn = state.conn.lock().unwrap();
    db::mark_used(&conn, &w.id, Utc::now().timestamp())?;
    let mut s = db::load_settings(&conn)?;
    s.current_wallpaper_id = Some(w.id.clone());
    db::save_settings(&conn, &s)?;
    Ok(())
}

/// One request for metadata, one request for the image. Nothing else.
async fn try_download(
    state: &AppState,
    settings: &Settings,
    exclude: Vec<String>,
) -> Result<Wallpaper> {
    let provider = Provider::from_id(&settings.provider);
    if !provider.requires_api_key() {
        return Err(anyhow!("the local provider never downloads"));
    }
    let api_key = secrets::get_api_key(&settings.provider)
        .ok_or_else(|| anyhow!("no API key set for {}", settings.provider))?;

    let (screen_width, screen_height) = platform::primary_resolution();
    let req = FetchRequest {
        category: settings.category.clone(),
        query: settings.search_query.clone(),
        width: screen_width,
        height: screen_height,
        exclude_remote_ids: exclude,
    };

    state.set_network_active(true);
    let result = download_inner(state, &provider, &api_key, &req).await;
    state.set_network_active(false);
    result
}

async fn download_inner(
    state: &AppState,
    provider: &Provider,
    api_key: &str,
    req: &FetchRequest,
) -> Result<Wallpaper> {
    let remote = provider.fetch_one(&state.client, api_key, req).await?;

    {
        let conn = state.conn.lock().unwrap();
        if db::has_remote_id(&conn, provider.id(), &remote.remote_id)? {
            return Err(anyhow!("provider offered a wallpaper we already have"));
        }
    }

    if !remote.download_url.starts_with("https://") {
        return Err(anyhow!("refusing a non-HTTPS image URL"));
    }

    let resp = state.client.get(&remote.download_url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!("image download returned {}", resp.status()));
    }

    // Reject oversized bodies before reading them, when the server declares a length.
    if let Some(len) = resp.content_length() {
        if len > storage::MAX_IMAGE_BYTES {
            return Err(anyhow!("image is larger than the {} MB limit", storage::MAX_IMAGE_BYTES / 1024 / 1024));
        }
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !content_type.starts_with("image/") {
        return Err(anyhow!("server did not return image data"));
    }

    let bytes = resp.bytes().await?;
    if bytes.len() as u64 > storage::MAX_IMAGE_BYTES {
        return Err(anyhow!("image exceeded the size limit while downloading"));
    }

    // A wallpaper must at least cover the screen, or Windows upscales it and it
    // looks soft. Allow a small tolerance for the CDN rounding a crop by a pixel.
    let min_width = req.width.saturating_sub(2);
    let min_height = req.height.saturating_sub(2);
    let image = storage::validate_and_normalize(&bytes, min_width, min_height)?;

    let file_name = storage::build_file_name(provider.id(), &remote.remote_id);
    let path = state.paths.wallpaper_path(&file_name)?;
    storage::write_atomic(&path, &image.jpeg)?;

    if let Some(trigger) = &remote.download_trigger_url {
        provider.notify_download(&state.client, api_key, trigger).await;
    }

    let wallpaper = Wallpaper {
        id: format!("{}:{}", provider.id(), remote.remote_id),
        provider: provider.id().to_string(),
        remote_id: remote.remote_id,
        file_name,
        width: image.width,
        height: image.height,
        source_width: remote.source_width,
        source_height: remote.source_height,
        format: image.format.to_string(),
        bytes: image.jpeg.len() as u64,
        downloaded_at: Utc::now().timestamp(),
        // Record what was actually searched for, which is the user's query when
        // they gave one — otherwise history would credit the wrong term.
        category: req.search_terms(),
        author_name: remote.author_name,
        author_url: remote.author_url,
        source_url: remote.source_url,
        is_favorite: false,
        last_used_at: None,
    };

    {
        let conn = state.conn.lock().unwrap();
        db::insert_wallpaper(&conn, &wallpaper)?;
    }
    enforce_storage_limits(state)?;
    Ok(wallpaper)
}

/// Never deletes the current wallpaper, the user's default, or a favorite.
pub fn enforce_storage_limits(state: &AppState) -> Result<()> {
    let conn = state.conn.lock().unwrap();
    let settings = db::load_settings(&conn)?;
    let mut protected: Vec<String> = Vec::new();
    if let Some(id) = &settings.current_wallpaper_id {
        protected.push(id.clone());
    }

    let (mut used, mut count) = db::storage_stats(&conn)?;
    let max_bytes = settings.max_storage_mb * 1024 * 1024;

    let candidates = db::eviction_candidates(&conn, &protected)?;
    for w in candidates {
        if used <= max_bytes && count <= settings.keep_recent {
            break;
        }
        let path = state.paths.wallpaper_path(&w.file_name)?;
        let _ = std::fs::remove_file(&path);
        db::delete_wallpaper(&conn, &w.id)?;
        used = used.saturating_sub(w.bytes);
        count = count.saturating_sub(1);
    }
    Ok(())
}

/// A change is due when the scheduled time has passed. There is no polling of
/// the API — this only decides whether the *next* wallpaper may come from the
/// network, and a recent failure keeps us offline until the next slot.
fn should_attempt_online(state: &AppState, settings: &Settings) -> bool {
    if Provider::from_id(&settings.provider).id() == "local" {
        return false;
    }
    if !secrets::has_api_key(&settings.provider) {
        return false;
    }
    let rt = state.runtime.lock().unwrap();
    match rt.online_attempt_failed_at {
        // One failure parks online attempts for the rest of this interval.
        Some(ts) => Utc::now().timestamp() - ts > (settings.interval_hours as i64 * 3600),
        None => true,
    }
}

pub fn schedule_next(state: &AppState, settings: &Settings) -> Result<()> {
    let next = compute_next_change(settings, Utc::now().timestamp());
    let conn = state.conn.lock().unwrap();
    let mut s = db::load_settings(&conn)?;
    s.next_change_at = next;
    db::save_settings(&conn, &s)?;
    Ok(())
}

/// Daily schedules land on the user's preferred local hour; shorter intervals
/// simply add the interval to now.
pub fn compute_next_change(settings: &Settings, now_ts: i64) -> i64 {
    let now = Local
        .timestamp_opt(now_ts, 0)
        .single()
        .unwrap_or_else(Local::now);

    if settings.interval_hours >= 24 {
        let target_hour = settings.preferred_hour.min(23);
        let mut candidate = now
            .date_naive()
            .and_hms_opt(target_hour, 0, 0)
            .map(|d| Local.from_local_datetime(&d).single().unwrap_or(now))
            .unwrap_or(now);
        let days = (settings.interval_hours / 24).max(1) as i64;
        while candidate <= now {
            candidate += Duration::days(days);
        }
        candidate.timestamp()
    } else {
        let hours = settings.interval_hours.max(1) as i64;
        (now + Duration::hours(hours))
            .with_minute(0)
            .unwrap_or(now)
            .timestamp()
    }
}
