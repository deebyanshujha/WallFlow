use super::{pick_random, random_below, resolution_bucket, urlencode, NothingNew, WallpaperProvider};
use crate::models::{FetchRequest, RemoteWallpaper};
use anyhow::{anyhow, Result};
use serde::Deserialize;

pub struct PexelsProvider;

/// Photos per page. Big enough to almost always hold something unseen, small
/// enough that the JSON stays a few dozen KB.
const PER_PAGE: usize = 30;
/// Our first guess at how deep to page before we know the result count.
const FIRST_GUESS_PAGES: usize = 30;

#[derive(Deserialize)]
struct PexelsSearch {
    photos: Vec<PexelsPhoto>,
    #[serde(default)]
    total_results: usize,
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
        // Pexels has no random endpoint, so we jump to a random page. We do
        // not know how many pages a search has until we ask, so if the guess
        // overshoots, the reply's total tells us the real range and one more
        // request lands on a random page inside it.
        let mut body = search_page(client, api_key, req, 1 + random_below(FIRST_GUESS_PAGES)).await?;
        if body.photos.is_empty() && body.total_results > 0 {
            let pages = body.total_results.div_ceil(PER_PAGE).max(1);
            body = search_page(client, api_key, req, 1 + random_below(pages)).await?;
        }
        if body.photos.is_empty() {
            return Err(anyhow!("Pexels found nothing for \"{}\"", req.terms.trim()));
        }
        let total = body.photos.len();

        let unseen: Vec<PexelsPhoto> = body
            .photos
            .into_iter()
            .filter(|p| !req.exclude_remote_ids.contains(&p.id.to_string()))
            .filter(|p| p.width >= p.height)
            .collect();
        let photo = pick_random(&unseen, |p| p.width >= req.width && p.height >= req.height)
            .ok_or_else(|| NothingNew(format!("all {total} Pexels photos on this page were already shown")))?;

        let bucket = resolution_bucket(req.width);
        let download_url = if bucket <= 1920 {
            photo
                .src
                .large2x
                .clone()
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
            author_name: photo.photographer.clone(),
            author_url: photo.photographer_url.clone(),
            source_url: photo.url.clone(),
            download_trigger_url: None,
        })
    }
}

/// One metadata request. Empty terms use the curated feed, since Pexels
/// search requires a query.
async fn search_page(
    client: &reqwest::Client,
    api_key: &str,
    req: &FetchRequest,
    page: usize,
) -> Result<PexelsSearch> {
    let url = match req.terms.trim() {
        "" => format!("https://api.pexels.com/v1/curated?per_page={PER_PAGE}&page={page}"),
        t => format!(
            "https://api.pexels.com/v1/search?query={}&orientation=landscape&per_page={PER_PAGE}&page={page}",
            urlencode(t)
        ),
    };
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
    Ok(resp.json().await?)
}
