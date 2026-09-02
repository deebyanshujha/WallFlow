use super::WallpaperProvider;
use crate::models::{FetchRequest, RemoteWallpaper};
use anyhow::{anyhow, Result};

/// Offline-only provider. It never reaches the network; selecting it means the
/// app runs purely off the local cache and the user's default wallpaper.
pub struct LocalProvider;

impl WallpaperProvider for LocalProvider {
    fn id(&self) -> &'static str {
        "local"
    }
    fn requires_api_key(&self) -> bool {
        false
    }

    async fn fetch_one(
        &self,
        _client: &reqwest::Client,
        _api_key: &str,
        _req: &FetchRequest,
    ) -> Result<RemoteWallpaper> {
        Err(anyhow!("local provider has nothing to download"))
    }
}
