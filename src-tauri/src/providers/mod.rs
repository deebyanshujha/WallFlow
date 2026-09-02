pub mod local;
pub mod pexels;
pub mod unsplash;

use crate::models::{FetchRequest, RemoteWallpaper};
use anyhow::Result;

/// A source of wallpapers. The rest of the app only ever talks to this.
pub trait WallpaperProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// True if the provider needs a user-supplied API key to work.
    fn requires_api_key(&self) -> bool {
        true
    }

    /// Pick exactly one candidate. Implementations must request the smallest
    /// image that satisfies the requested resolution — never the raw original.
    fn fetch_one(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        req: &FetchRequest,
    ) -> impl std::future::Future<Output = Result<RemoteWallpaper>> + Send;
}

/// Dispatch enum instead of `dyn` so the async trait method stays object-safe-free
/// and monomorphized; adding a provider means adding one variant.
pub enum Provider {
    Unsplash(unsplash::UnsplashProvider),
    Pexels(pexels::PexelsProvider),
    Local(local::LocalProvider),
}

impl Provider {
    pub fn from_id(id: &str) -> Self {
        match id {
            "pexels" => Provider::Pexels(pexels::PexelsProvider),
            "local" => Provider::Local(local::LocalProvider),
            _ => Provider::Unsplash(unsplash::UnsplashProvider),
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Provider::Unsplash(p) => p.id(),
            Provider::Pexels(p) => p.id(),
            Provider::Local(p) => p.id(),
        }
    }

    pub fn requires_api_key(&self) -> bool {
        match self {
            Provider::Unsplash(p) => p.requires_api_key(),
            Provider::Pexels(p) => p.requires_api_key(),
            Provider::Local(p) => p.requires_api_key(),
        }
    }

    /// Best-effort courtesy call some providers require once an image is used.
    /// Failure is ignored: it must never stop a wallpaper from being applied.
    pub async fn notify_download(&self, client: &reqwest::Client, api_key: &str, url: &str) {
        if !url.starts_with("https://") {
            return;
        }
        let request = match self {
            Provider::Unsplash(_) => client
                .get(url)
                .header("Authorization", format!("Client-ID {api_key}")),
            _ => return,
        };
        let _ = request.send().await;
    }

    pub async fn fetch_one(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        req: &FetchRequest,
    ) -> Result<RemoteWallpaper> {
        match self {
            Provider::Unsplash(p) => p.fetch_one(client, api_key, req).await,
            Provider::Pexels(p) => p.fetch_one(client, api_key, req).await,
            Provider::Local(p) => p.fetch_one(client, api_key, req).await,
        }
    }
}

/// Round a screen dimension up to a sane request bucket so we ask providers for
/// a 1920px image on a 1080p display instead of pulling a 6000px original.
pub fn resolution_bucket(width: u32) -> u32 {
    const BUCKETS: [u32; 5] = [1280, 1600, 1920, 2560, 3840];
    for b in BUCKETS {
        if width <= b {
            return b;
        }
    }
    3840
}
