use crate::models::{AppStatus, FetchRequest, Settings, Wallpaper};
use crate::providers::{NothingNew, Provider};
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
        let history_bytes = db::history_bytes(&conn)?;
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
            history_bytes,
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
        let e = db::known_remote_ids(&conn, &s.provider, SEEN_HISTORY)?;
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
        // Cached wallpapers are tagged with what was searched for, so match
        // on the same terms rather than the category alone.
        db::pick_cached(
            &conn,
            &settings.search_terms(),
            settings.current_wallpaper_id.as_deref(),
            settings.allow_nsfw,
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

/// Called when NSFW is switched off: if the desktop is showing an NSFW
/// wallpaper, swap it for a safe cached one (or the default) right away,
/// without touching the network.
pub fn replace_if_nsfw(state: &AppState, settings: &Settings) -> Result<()> {
    if settings.allow_nsfw {
        return Ok(());
    }
    let replacement = {
        let conn = state.conn.lock().unwrap();
        let current = match &settings.current_wallpaper_id {
            Some(id) => db::get_wallpaper(&conn, id)?,
            None => None,
        };
        if !current.is_some_and(|w| w.nsfw) {
            return Ok(());
        }
        db::pick_cached(&conn, &settings.search_terms(), None, false)?
    };
    match replacement {
        Some(w) => apply(state, &w),
        None => apply_default(state, settings),
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

/// How many past downloads per provider are never offered again. Large enough
/// that repeats are effectively gone; bounded so a narrow search can recycle
/// its oldest results instead of running dry forever.
const SEEN_HISTORY: usize = 2000;

/// Metadata requests allowed per change when a provider only offers images we
/// have already shown. Each retry is a fresh random sample.
const FETCH_ATTEMPTS: usize = 3;

/// One request for metadata (a couple more only if every candidate was a
/// repeat), one request for the image. Nothing else.
async fn try_download(
    state: &AppState,
    settings: &Settings,
    exclude: Vec<String>,
) -> Result<Wallpaper> {
    let provider = Provider::from_id(&settings.provider);
    if !provider.downloads() {
        return Err(anyhow!("the local provider never downloads"));
    }
    let api_key = match secrets::get_api_key(&settings.provider) {
        Some(k) => k,
        None if !provider.requires_api_key() => String::new(),
        None => return Err(anyhow!("no API key set for {}", settings.provider)),
    };

    let (screen_width, screen_height) = platform::primary_resolution();
    let req = FetchRequest {
        terms: settings.search_terms(),
        width: screen_width,
        height: screen_height,
        exclude_remote_ids: exclude,
        allow_nsfw: settings.allow_nsfw,
    };

    state.set_network_active(true);
    let mut result = download_inner(state, &provider, &api_key, &req).await;
    for _ in 1..FETCH_ATTEMPTS {
        match &result {
            Err(e) if e.is::<NothingNew>() => {
                result = download_inner(state, &provider, &api_key, &req).await;
            }
            _ => break,
        }
    }
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
            return Err(NothingNew("provider offered a wallpaper we already have".into()).into());
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
        category: if req.terms.trim().is_empty() {
            "anything".to_string()
        } else {
            req.terms.trim().to_string()
        },
        author_name: remote.author_name,
        author_url: remote.author_url,
        source_url: remote.source_url,
        is_favorite: false,
        last_used_at: None,
        nsfw: remote.nsfw,
        trashed_at: None,
    };

    {
        let conn = state.conn.lock().unwrap();
        db::insert_wallpaper(&conn, &wallpaper)?;
    }
    // It is not current until apply() runs, so protect it explicitly or a full
    // cache would evict the very file we are about to set.
    enforce_storage_limits(state, Some(&wallpaper.id))?;
    Ok(wallpaper)
}

/// Trims History to the keep-recent window and the size budget. Never deletes
/// the current wallpaper, the user's default, a favorite, the trash, or `keep`.
pub fn enforce_storage_limits(state: &AppState, keep: Option<&str>) -> Result<()> {
    let conn = state.conn.lock().unwrap();
    let settings = db::load_settings(&conn)?;
    let mut protected: Vec<String> = Vec::new();
    if let Some(id) = &settings.current_wallpaper_id {
        protected.push(id.clone());
    }
    if let Some(id) = keep {
        protected.push(id.to_string());
    }

    let recent: Vec<String> = db::list_history(&conn, settings.keep_recent)?
        .into_iter()
        .map(|w| w.id)
        .collect();
    let mut used = db::history_bytes(&conn)?;
    let max_bytes = settings.max_storage_mb * 1024 * 1024;

    let candidates = db::eviction_candidates(&conn, &protected)?;
    for w in candidates {
        // Oldest first, so everything outside the window goes before anything in it.
        if used <= max_bytes && recent.contains(&w.id) {
            break;
        }
        let path = state.paths.wallpaper_path(&w.file_name)?;
        let _ = std::fs::remove_file(&path);
        db::delete_wallpaper(&conn, &w.id)?;
        used = used.saturating_sub(w.bytes);
    }
    Ok(())
}

/// How long a removed favorite waits in the trash before it is deleted for good.
pub const TRASH_RETENTION_SECS: i64 = 30 * 24 * 3600;

/// Permanently deletes trashed wallpapers, or only those past `TRASH_RETENTION_SECS`
/// when `expired_only` is set. The current wallpaper's file is still in use, so it
/// stays until something replaces it. Returns how many were deleted.
pub fn purge_trash(state: &AppState, expired_only: bool) -> Result<usize> {
    let conn = state.conn.lock().unwrap();
    let settings = db::load_settings(&conn)?;
    let cutoff = expired_only.then(|| Utc::now().timestamp() - TRASH_RETENTION_SECS);
    let mut removed = 0;
    for w in db::trash_candidates(&conn, cutoff)? {
        if settings.current_wallpaper_id.as_deref() == Some(w.id.as_str()) {
            continue;
        }
        let path = state.paths.wallpaper_path(&w.file_name)?;
        let _ = std::fs::remove_file(&path);
        db::delete_wallpaper(&conn, &w.id)?;
        removed += 1;
    }
    Ok(removed)
}

/// A change is due when the scheduled time has passed. There is no polling of
/// the API — this only decides whether the *next* wallpaper may come from the
/// network, and a recent failure keeps us offline until the next slot.
fn should_attempt_online(state: &AppState, settings: &Settings) -> bool {
    let provider = Provider::from_id(&settings.provider);
    if !provider.downloads() {
        return false;
    }
    if provider.requires_api_key() && !secrets::has_api_key(&settings.provider) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(name: &str) -> AppState {
        let root = std::env::temp_dir().join(format!("wallflow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = storage::Paths::new(root).unwrap();
        let conn = db::open(&paths.db).unwrap();
        AppState {
            paths,
            conn: Mutex::new(conn),
            client: reqwest::Client::new(),
            runtime: Mutex::new(Runtime::default()),
            bundled_default: PathBuf::new(),
        }
    }

    /// Adds a wallpaper whose age order follows `n`: higher is newer.
    fn add(state: &AppState, n: i64) -> String {
        let id = format!("test:{n}");
        let w = Wallpaper {
            id: id.clone(),
            provider: "test".into(),
            remote_id: n.to_string(),
            file_name: format!("test-{n}.jpg"),
            width: 1,
            height: 1,
            source_width: 0,
            source_height: 0,
            format: String::new(),
            bytes: 1,
            downloaded_at: n,
            category: "nature".into(),
            author_name: None,
            author_url: None,
            source_url: None,
            is_favorite: false,
            last_used_at: None,
            nsfw: false,
            trashed_at: None,
        };
        db::insert_wallpaper(&state.conn.lock().unwrap(), &w).unwrap();
        id
    }

    fn ids(list: Vec<Wallpaper>) -> Vec<String> {
        list.into_iter().map(|w| w.id).collect()
    }

    #[test]
    fn favorites_outlive_the_history_window_and_do_not_shrink_it() {
        let state = test_state("window");
        let favorite = add(&state, 1);
        db::add_favorite(&state.conn.lock().unwrap(), &favorite).unwrap();
        for n in 2..=20 {
            add(&state, n);
            enforce_storage_limits(&state, None).unwrap();
        }

        let conn = state.conn.lock().unwrap();
        let history = ids(db::list_history(&conn, 7).unwrap());
        assert_eq!(history.len(), 7);
        assert_eq!(history[0], "test:20");
        assert_eq!(history[6], "test:14");
        assert!(!history.contains(&favorite));
        assert_eq!(ids(db::list_favorites(&conn).unwrap()), vec![favorite]);
        // Seven recents plus the favorite; everything else was evicted.
        assert_eq!(db::storage_stats(&conn).unwrap().1, 8);
    }

    #[test]
    fn the_size_budget_ignores_favorites_and_trash() {
        let state = test_state("budget");
        let favorite = add(&state, 1);
        let trashed = add(&state, 2);
        let recent = add(&state, 3);
        {
            let conn = state.conn.lock().unwrap();
            for id in [&favorite, &trashed] {
                db::add_favorite(&conn, id).unwrap();
            }
            db::trash_favorite(&conn, &trashed, 100).unwrap();
            assert_eq!(db::history_bytes(&conn).unwrap(), 1);
        }
        enforce_storage_limits(&state, None).unwrap();
        let conn = state.conn.lock().unwrap();
        assert!(db::get_wallpaper(&conn, &recent).unwrap().is_some());
    }

    #[test]
    fn removed_favorites_wait_in_trash_and_can_be_restored() {
        let state = test_state("restore");
        let id = add(&state, 1);
        let conn = state.conn.lock().unwrap();

        // Only an actual favorite can be sent to the trash.
        db::trash_favorite(&conn, &id, 100).unwrap();
        assert!(db::list_trash(&conn).unwrap().is_empty());

        db::add_favorite(&conn, &id).unwrap();
        db::trash_favorite(&conn, &id, 100).unwrap();
        assert!(db::list_favorites(&conn).unwrap().is_empty());
        assert!(db::list_history(&conn, 7).unwrap().is_empty());
        assert!(db::pick_cached(&conn, "", None, true).unwrap().is_none());
        assert_eq!(ids(db::list_trash(&conn).unwrap()), vec![id.clone()]);

        db::add_favorite(&conn, &id).unwrap();
        assert!(db::list_trash(&conn).unwrap().is_empty());
        assert_eq!(ids(db::list_favorites(&conn).unwrap()), vec![id]);
    }

    #[test]
    fn purge_respects_retention_and_the_current_wallpaper() {
        let state = test_state("purge");
        let now = Utc::now().timestamp();
        let expired = add(&state, 1);
        let fresh = add(&state, 2);
        let in_use = add(&state, 3);
        {
            let conn = state.conn.lock().unwrap();
            for id in [&expired, &fresh, &in_use] {
                db::add_favorite(&conn, id).unwrap();
            }
            db::trash_favorite(&conn, &expired, now - TRASH_RETENTION_SECS - 1).unwrap();
            db::trash_favorite(&conn, &fresh, now).unwrap();
            db::trash_favorite(&conn, &in_use, now - TRASH_RETENTION_SECS - 1).unwrap();
            let mut s = db::load_settings(&conn).unwrap();
            s.current_wallpaper_id = Some(in_use.clone());
            db::save_settings(&conn, &s).unwrap();
        }

        assert_eq!(purge_trash(&state, true).unwrap(), 1);
        {
            let conn = state.conn.lock().unwrap();
            let mut left = ids(db::list_trash(&conn).unwrap());
            left.sort();
            assert_eq!(left, vec![fresh.clone(), in_use.clone()]);
        }

        // Clearing by hand ignores the age but still keeps what is on screen.
        assert_eq!(purge_trash(&state, false).unwrap(), 1);
        let conn = state.conn.lock().unwrap();
        assert_eq!(ids(db::list_trash(&conn).unwrap()), vec![in_use]);
    }
}
