use super::WallpaperProvider;
use crate::models::{FetchRequest, RemoteWallpaper};
use anyhow::{anyhow, Result};
use serde::Deserialize;

pub struct UnsplashProvider;

/// How many candidates to ask for in the one metadata request. Enough to
/// almost always contain something new without bloating the response.
const CANDIDATE_COUNT: u32 = 10;

#[derive(Deserialize)]
struct UnsplashPhoto {
    id: String,
    width: u32,
    height: u32,
    urls: UnsplashUrls,
    links: Option<UnsplashLinks>,
    user: Option<UnsplashUser>,
}

#[derive(Deserialize)]
struct UnsplashUrls {
    raw: String,
}

#[derive(Deserialize)]
struct UnsplashLinks {
    html: Option<String>,
    download_location: Option<String>,
}

#[derive(Deserialize)]
struct UnsplashUser {
    name: Option<String>,
    links: Option<UnsplashLinks>,
}

impl WallpaperProvider for UnsplashProvider {
    fn id(&self) -> &'static str {
        "unsplash"
    }

    async fn fetch_one(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        req: &FetchRequest,
    ) -> Result<RemoteWallpaper> {
        if api_key.is_empty() {
            return Err(anyhow!("no Unsplash API key configured"));
        }
        // Still a single API request, but asking for a handful of candidates
        // instead of one. `photos/random` with a query samples from a small
        // pool, so a lone result is very often one we already own — and we
        // would then fall back to the cache and never see anything new.
        // The extra cost is a few KB of JSON; we still download one image.
        let url = format!(
            "https://api.unsplash.com/photos/random?query={}&orientation=landscape&content_filter=high&count={CANDIDATE_COUNT}",
            urlencode(&req.search_terms())
        );
        let resp = client
            .get(&url)
            .header("Authorization", format!("Client-ID {api_key}"))
            .header("Accept-Version", "v1")
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(anyhow!("rate limited by Unsplash"));
        }
        if !resp.status().is_success() {
            return Err(anyhow!("Unsplash returned {}", resp.status()));
        }

        // With `count` the endpoint returns an array rather than an object.
        let photos: Vec<UnsplashPhoto> = resp.json().await?;
        let total = photos.len();

        // Pick from the candidates we already have: unseen, and big enough to
        // cover the screen without being upscaled. Only the winner is
        // downloaded — the rest cost nothing beyond their few lines of JSON.
        let unseen: Vec<UnsplashPhoto> = photos
            .into_iter()
            .filter(|p| !req.exclude_remote_ids.contains(&p.id))
            .collect();
        if unseen.is_empty() {
            return Err(anyhow!(
                "all {total} Unsplash candidates were already in the cache"
            ));
        }

        let photo = unseen
            .iter()
            .find(|p| p.width >= req.width && p.height >= req.height)
            // Nothing covers the screen outright, so take the largest we were
            // offered rather than giving up on a new wallpaper entirely.
            .or_else(|| unseen.iter().max_by_key(|p| p.width as u64 * p.height as u64))
            .ok_or_else(|| anyhow!("no usable Unsplash candidate"))?;

        Ok(RemoteWallpaper {
            download_url: build_download_url(&photo.urls.raw, req.width, req.height),
            remote_id: photo.id.clone(),
            source_width: photo.width,
            source_height: photo.height,
            author_name: photo.user.as_ref().and_then(|u| u.name.clone()),
            author_url: photo
                .user
                .as_ref()
                .and_then(|u| u.links.as_ref())
                .and_then(|l| l.html.clone()),
            source_url: photo.links.as_ref().and_then(|l| l.html.clone()),
            download_trigger_url: photo
                .links
                .as_ref()
                .and_then(|l| l.download_location.clone()),
        })
    }
}

/// Builds the CDN URL for exactly the pixels the desktop needs.
///
/// `fit=crop` makes Unsplash crop to the display's aspect ratio server-side, so
/// the image arrives at the exact screen size: Windows never has to rescale it
/// (which is what softened it before) and never stretches it. `q=90` is high
/// enough that the JPEG is visually clean at full screen while staying a few
/// hundred KB. Existing query parameters, including the required `ixid`, are
/// preserved — ours are appended with the correct separator.
fn build_download_url(raw: &str, width: u32, height: u32) -> String {
    let separator = if raw.contains('?') { '&' } else { '?' };
    format!("{raw}{separator}w={width}&h={height}&fit=crop&crop=entropy&q=90&fm=jpg")
}

fn urlencode(s: &str) -> String {
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
    fn keeps_existing_query_params_including_ixid() {
        let url = build_download_url(
            "https://images.unsplash.com/photo-1?ixid=ABC&ixlib=rb-4.1.0",
            2560,
            1440,
        );
        assert!(url.contains("ixid=ABC"), "ixid must survive: {url}");
        assert!(url.contains("&w=2560&h=1440"));
        assert!(url.contains("fit=crop"));
        assert!(url.contains("q=90"));
    }

    #[test]
    fn adds_a_query_string_when_the_raw_url_has_none() {
        let url = build_download_url("https://images.unsplash.com/photo-2", 1920, 1080);
        assert!(url.contains("photo-2?w=1920"), "must start a query: {url}");
        assert!(!url.contains("photo-2&"), "must not append with &: {url}");
    }

    #[test]
    fn requests_the_actual_display_size_not_a_fixed_one() {
        for (w, h) in [(1920, 1080), (2560, 1440), (3840, 2160)] {
            let url = build_download_url("https://images.unsplash.com/p?ixid=X", w, h);
            assert!(url.contains(&format!("w={w}&h={h}")), "{url}");
        }
    }

    #[test]
    fn search_query_wins_over_category_but_category_is_the_fallback() {
        let mut req = crate::models::FetchRequest {
            category: "nature".into(),
            query: "  blue cars  ".into(),
            width: 1920,
            height: 1080,
            exclude_remote_ids: vec![],
        };
        assert_eq!(req.search_terms(), "blue cars");
        req.query = "   ".into();
        assert_eq!(req.search_terms(), "nature");
    }
}
