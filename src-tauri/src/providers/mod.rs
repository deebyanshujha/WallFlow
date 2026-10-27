pub mod local;
pub mod pexels;
pub mod unsplash;
pub mod wallhaven;

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
    Wallhaven(wallhaven::WallhavenProvider),
    Local(local::LocalProvider),
}

impl Provider {
    pub fn from_id(id: &str) -> Self {
        match id {
            "pexels" => Provider::Pexels(pexels::PexelsProvider),
            "wallhaven" => Provider::Wallhaven(wallhaven::WallhavenProvider),
            "local" => Provider::Local(local::LocalProvider),
            _ => Provider::Unsplash(unsplash::UnsplashProvider),
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Provider::Unsplash(p) => p.id(),
            Provider::Pexels(p) => p.id(),
            Provider::Wallhaven(p) => p.id(),
            Provider::Local(p) => p.id(),
        }
    }

    /// False only for the offline provider, which never touches the network.
    pub fn downloads(&self) -> bool {
        !matches!(self, Provider::Local(_))
    }

    pub fn requires_api_key(&self) -> bool {
        match self {
            Provider::Unsplash(p) => p.requires_api_key(),
            Provider::Pexels(p) => p.requires_api_key(),
            Provider::Wallhaven(p) => p.requires_api_key(),
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
            Provider::Wallhaven(p) => p.fetch_one(client, api_key, req).await,
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

/// Returned when a provider answered fine but every candidate was one we have
/// already shown. The engine treats it as "ask again", unlike real failures.
#[derive(Debug)]
pub struct NothingNew(pub String);

impl std::fmt::Display for NothingNew {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NothingNew {}

/// A fresh random number from the OS-seeded hasher std uses for HashMap, so
/// we get real randomness without pulling in a crate for it.
pub fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.finish()
}

/// Random index in `0..n`; `n` must be non-zero.
pub fn random_below(n: usize) -> usize {
    (random_u64() % n as u64) as usize
}

/// Picks one element at random, preferring those that satisfy `better` (e.g.
/// big enough to cover the screen) and falling back to the whole slice.
pub fn pick_random<T>(items: &[T], better: impl Fn(&T) -> bool) -> Option<&T> {
    let preferred: Vec<&T> = items.iter().filter(|i| better(i)).collect();
    if !preferred.is_empty() {
        return Some(preferred[random_below(preferred.len())]);
    }
    if items.is_empty() {
        None
    } else {
        Some(&items[random_below(items.len())])
    }
}

pub(crate) fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            other => other
                .to_string()
                .bytes()
                .map(|b| format!("%{b:02X}"))
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_random_prefers_matching_items_and_varies() {
        let items: Vec<u32> = (0..20).collect();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let v = *pick_random(&items, |v| v % 2 == 0).unwrap();
            assert_eq!(v % 2, 0);
            seen.insert(v);
        }
        assert!(seen.len() > 5, "picks should spread out: {seen:?}");
        assert_eq!(pick_random(&items, |_| false).map(|v| *v < 20), Some(true));
        assert!(pick_random::<u32>(&[], |_| true).is_none());
    }
}
