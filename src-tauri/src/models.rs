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
    /// Either "category" (use `category`) or "search" (use `search_query`).
    /// Empty only for settings saved before the choice existed; `db` fills it in.
    #[serde(default)]
    pub source_mode: String,
    /// Preset category. Empty means "anything" — the provider's random pool.
    pub category: String,
    /// Free-text wallpaper search, used only in "search" mode.
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
            // Works out of the box: Wallhaven needs no key for SFW wallpapers.
            provider: "wallhaven".into(),
            source_mode: "category".into(),
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

impl Settings {
    /// What we actually ask the provider for: the user's own words in search
    /// mode, otherwise the category they picked. Empty means "anything".
    /// A search with nothing typed falls back to the category rather than
    /// silently turning into "anything".
    pub fn search_terms(&self) -> String {
        let q = self.search_query.trim();
        if self.source_mode == "search" && !q.is_empty() {
            q.to_string()
        } else {
            self.category.trim().to_string()
        }
    }

    /// Settings saved before `source_mode` existed treated a non-empty query
    /// as a search, so keep that meaning for them.
    pub fn normalize(&mut self) {
        if self.source_mode != "category" && self.source_mode != "search" {
            self.source_mode = if self.search_query.trim().is_empty() {
                "category".into()
            } else {
                "search".into()
            };
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
    /// Keywords to search for, from `Settings::search_terms`. Empty means
    /// "anything": the provider's general random pool.
    pub terms: String,
    pub width: u32,
    pub height: u32,
    pub exclude_remote_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_decides_between_search_and_category() {
        let mut s = Settings {
            category: "nature".into(),
            search_query: "  blue cars  ".into(),
            ..Settings::default()
        };
        assert_eq!(s.search_terms(), "nature", "category mode ignores the query");
        s.source_mode = "search".into();
        assert_eq!(s.search_terms(), "blue cars");
        s.search_query = "   ".into();
        assert_eq!(s.search_terms(), "nature", "empty search falls back");
        s.category = String::new();
        assert_eq!(s.search_terms(), "", "empty category means anything");
    }

    #[test]
    fn legacy_settings_with_a_query_become_search_mode() {
        let mut s = Settings {
            source_mode: String::new(),
            search_query: "cars".into(),
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(s.source_mode, "search");
        let mut s = Settings {
            source_mode: String::new(),
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(s.source_mode, "category");
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
