use super::{resolution_bucket, WallpaperProvider};
use crate::models::{FetchRequest, RemoteWallpaper};
use anyhow::{anyhow, Result};
use serde::Deserialize;

pub struct PexelsProvider;

#[derive(Deserialize)]
struct PexelsSearch {
    photos: Vec<PexelsPhoto>,
}

#[derive(Deserialize)]
struct PexelsPhoto {
    id: u64,
    width: u32,
    height: u32,
    url: Option<String>,
    photographer: Option<String>,
    photographer_url: Option<String>,
    src: PexelsSrc,
}

#[derive(Deserialize)]
struct PexelsSrc {
    large2x: Option<String>,
    large: Option<String>,
    original: String,
}

impl WallpaperProvider for PexelsProvider {
    fn id(&self) -> &'static str {
        "pexels"
    }

    async fn fetch_one(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        req: &FetchRequest,
    ) -> Result<RemoteWallpaper> {
        if api_key.is_empty() {
            return Err(anyhow!("no Pexels API key configured"));
        }
        // Pexels has no random endpoint, so we page pseudo-randomly with a small
        // per_page to keep the JSON payload tiny.
        let page = 1 + (now_seed() % 20);
        let url = format!(
            "https://api.pexels.com/v1/search?query={}&orientation=landscape&per_page=5&page={page}",
            urlencode(&req.search_terms())
        );
        let resp = client
            .get(&url)
            .header("Authorization", api_key)
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(anyhow!("rate limited by Pexels"));
        }
        if !resp.status().is_success() {
            return Err(anyhow!("Pexels returned {}", resp.status()));
        }

        let body: PexelsSearch = resp.json().await?;
        let photo = body
            .photos
            .into_iter()
            .find(|p| !req.exclude_remote_ids.contains(&p.id.to_string()))
            .ok_or_else(|| anyhow!("no new wallpapers in this Pexels page"))?;

        let bucket = resolution_bucket(req.width);
        let download_url = if bucket <= 1920 {
            photo
                .src
                .large2x
                .or(photo.src.large.clone())
                .unwrap_or(photo.src.original.clone())
        } else {
            photo.src.original.clone()
        };

        Ok(RemoteWallpaper {
            remote_id: photo.id.to_string(),
            download_url,
            source_width: photo.width,
            source_height: photo.height,
            author_name: photo.photographer,
            author_url: photo.photographer_url,
            source_url: photo.url,
            download_trigger_url: None,
        })
    }
}

fn now_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
