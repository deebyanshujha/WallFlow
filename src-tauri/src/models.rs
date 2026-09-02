use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallpaper {
    pub id: String,
    pub provider: String,
    pub remote_id: String,
    pub file_name: String,
    /// Dimensions of the file we actually stored.
    pub width: u32,
    pub height: u32,
    /// Dimensions of the original photo at the source, for reference in history.
    #[serde(default)]
    pub source_width: u32,
    #[serde(default)]
    pub source_height: u32,
    #[serde(default)]
    pub format: String,
    pub bytes: u64,
    pub downloaded_at: i64,
    pub category: String,
    pub author_name: Option<String>,
    pub author_url: Option<String>,
    pub source_url: Option<String>,
    pub is_favorite: bool,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub provider: String,
    pub category: String,
    /// Free-text wallpaper search. Empty means "just use the category".
    #[serde(default)]
    pub search_query: String,
    pub interval_hours: u32,
    pub preferred_hour: u32,
    pub keep_recent: u32,
    pub max_storage_mb: u64,
    pub start_with_windows: bool,
    pub theme: String,
    pub default_wallpaper_path: Option<String>,
    pub next_change_at: i64,
    pub current_wallpaper_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "unsplash".into(),
            category: "nature".into(),
            search_query: String::new(),
            interval_hours: 24,
            preferred_hour: 9,
            keep_recent: 7,
            max_storage_mb: 500,
            start_with_windows: false,
            theme: "system".into(),
            default_wallpaper_path: None,
            next_change_at: 0,
            current_wallpaper_id: None,
        }
    }
}

/// Snapshot handed to the UI on every poll — one round trip, no chatty IPC.
#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    pub settings: Settings,
    pub current: Option<Wallpaper>,
    pub network_active: bool,
    pub last_message: Option<String>,
    pub last_error: Option<String>,
    pub storage_used_bytes: u64,
    pub wallpaper_count: u32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub has_api_key: bool,
}

/// What a provider is asked for. Deliberately carries no user identity,
/// no local paths, and no history beyond the ids we must not receive again.
#[derive(Debug, Clone)]
pub struct FetchRequest {
    pub category: String,
    /// Free-text query the user typed. Takes precedence over the category.
    pub query: String,
    pub width: u32,
    pub height: u32,
    pub exclude_remote_ids: Vec<String>,
}

impl FetchRequest {
    /// What we actually ask the provider for: the user's own words when they
    /// gave any, otherwise the category they picked.
    pub fn search_terms(&self) -> String {
        let q = self.query.trim();
        if q.is_empty() {
            self.category.trim().to_string()
        } else {
            q.to_string()
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteWallpaper {
    pub remote_id: String,
    pub download_url: String,
    /// Size of the original photo at the source, before any server-side crop.
    pub source_width: u32,
    pub source_height: u32,
    pub author_name: Option<String>,
    pub author_url: Option<String>,
    pub source_url: Option<String>,
    /// Endpoint some providers require us to call once the image is actually
    /// used, so their download counts stay accurate (Unsplash API guidelines).
    pub download_trigger_url: Option<String>,
}
