use super::{pick_random, random_u64, urlencode, NothingNew, WallpaperProvider};
use crate::models::{FetchRequest, RemoteWallpaper};
use crate::storage::MAX_IMAGE_BYTES;
use anyhow::{anyhow, Result};
use serde::Deserialize;

/// https://wallhaven.cc/help/api — search works without a key for SFW
/// content; a key is only sent when the user stored one.
pub struct WallhavenProvider;

#[derive(Deserialize)]
struct WallhavenSearch {
    data: Vec<WallhavenWallpaper>,
}

#[derive(Deserialize)]
struct WallhavenWallpaper {
    id: String,
    url: Option<String>,
    dimension_x: u32,
    dimension_y: u32,
    #[serde(default)]
    file_size: u64,
    #[serde(default)]
    file_type: String,
    path: String,
}

impl WallpaperProvider for WallhavenProvider {
    fn id(&self) -> &'static str {
        "wallhaven"
    }
    fn requires_api_key(&self) -> bool {
        false
    }

    async fn fetch_one(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        req: &FetchRequest,
    ) -> Result<RemoteWallpaper> {
        let mut request = client
            .get(build_search_url(req, &random_seed()))
            .header("Accept", "application/json");
        if !api_key.is_empty() {
            request = request.header("X-API-Key", api_key);
        }
        let resp = request.send().await?;

        match resp.status() {
            s if s == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                return Err(anyhow!("rate limited by Wallhaven"))
            }
            s if s == reqwest::StatusCode::UNAUTHORIZED => {
                return Err(anyhow!("Wallhaven rejected the API key"))
            }
            s if !s.is_success() => return Err(anyhow!("Wallhaven returned {s}")),
            _ => {}
        }

        let body: WallhavenSearch = resp.json().await?;
        if body.data.is_empty() {
            return Err(anyhow!(
                "Wallhaven found nothing for \"{}\"",
                display_terms(req)
            ));
        }
        let total = body.data.len();

        // Wallhaven serves originals, so skip anything we could not store or
        // decode, and portrait shots that would be cropped to a sliver.
        let usable: Vec<WallhavenWallpaper> = body
            .data
            .into_iter()
            .filter(|w| !req.exclude_remote_ids.contains(&w.id))
            .filter(|w| w.path.starts_with("https://"))
            .filter(|w| matches!(w.file_type.as_str(), "image/jpeg" | "image/png" | ""))
            .filter(|w| w.file_size <= MAX_IMAGE_BYTES)
            .filter(|w| w.dimension_x >= w.dimension_y)
            .collect();
        if usable.is_empty() {
            return Err(NothingNew(format!(
                "all {total} Wallhaven results were already shown or unusable"
            ))
            .into());
        }

        // Prefer a shape close to the screen's so Windows' fill crops little.
        let screen_ratio = req.width as f64 / req.height.max(1) as f64;
        let photo = pick_random(&usable, |w| {
            let ratio = w.dimension_x as f64 / w.dimension_y.max(1) as f64;
            (ratio - screen_ratio).abs() < 0.15
        })
        .ok_or_else(|| anyhow!("no usable Wallhaven candidate"))?;

        Ok(RemoteWallpaper {
            remote_id: photo.id.clone(),
            download_url: photo.path.clone(),
            source_width: photo.dimension_x,
            source_height: photo.dimension_y,
            // Search results carry no uploader; the wallpaper page credits them.
            author_name: None,
            author_url: None,
            source_url: photo.url.clone(),
            download_trigger_url: None,
        })
    }
}

/// `sorting=random` with a fresh seed returns a newly shuffled page each
/// time; `atleast` makes the server drop anything smaller than the display.
/// Categories are general + anime + people, SFW only.
fn build_search_url(req: &FetchRequest, seed: &str) -> String {
    format!(
        "https://wallhaven.cc/api/v1/search?q={}&categories=111&purity=100&sorting=random&seed={seed}&atleast={}x{}",
        urlencode(req.terms.trim()),
        req.width,
        req.height
    )
}

/// Wallhaven wants exactly six alphanumerics.
fn random_seed() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut n = random_u64();
    (0..6)
        .map(|_| {
            let c = ALPHABET[(n % ALPHABET.len() as u64) as usize] as char;
            n /= ALPHABET.len() as u64;
            c
        })
        .collect()
}

fn display_terms(req: &FetchRequest) -> &str {
    match req.terms.trim() {
        "" => "anything",
        t => t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(terms: &str) -> FetchRequest {
        FetchRequest {
            terms: terms.into(),
            width: 2560,
            height: 1440,
            exclude_remote_ids: vec![],
        }
    }

    #[test]
    fn search_url_is_random_sfw_and_at_least_screen_size() {
        let url = build_search_url(&req(" blue cars "), "abc123");
        assert!(url.starts_with("https://wallhaven.cc/api/v1/search?"));
        assert!(url.contains("q=blue+cars&"), "{url}");
        assert!(url.contains("sorting=random&seed=abc123"), "{url}");
        assert!(url.contains("purity=100"), "{url}");
        assert!(url.contains("atleast=2560x1440"), "{url}");
    }

    #[test]
    fn empty_terms_search_everything() {
        assert!(build_search_url(&req(""), "abc123").contains("search?q=&"));
    }

    #[test]
    fn seeds_are_six_alphanumerics_and_vary() {
        let a = random_seed();
        assert_eq!(a.len(), 6);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!((0..10).any(|_| random_seed() != a));
    }

    #[test]
    fn parses_a_search_response() {
        let json = r#"{"data":[{"id":"94x38z","url":"https://wallhaven.cc/w/94x38z",
            "dimension_x":6742,"dimension_y":3534,"file_size":5070446,
            "file_type":"image/jpeg","path":"https://w.wallhaven.cc/full/94/wallhaven-94x38z.jpg",
            "thumbs":{"large":"x"}}],"meta":{"current_page":1,"seed":"abc123"}}"#;
        let body: WallhavenSearch = serde_json::from_str(json).unwrap();
        assert_eq!(body.data[0].id, "94x38z");
        assert_eq!(body.data[0].dimension_x, 6742);
    }
}
