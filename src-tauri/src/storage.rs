use anyhow::{anyhow, Result};
use image::ImageEncoder;
use std::path::{Path, PathBuf};

/// Hard ceiling on a single download. A provider (or a hijacked CDN response)
/// cannot make us stream an unbounded body onto disk.
pub const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024;
const MIN_IMAGE_BYTES: usize = 1024;

pub struct Paths {
    pub wallpapers: PathBuf,
    pub default_dir: PathBuf,
    pub db: PathBuf,
}

impl Paths {
    pub fn new(app_local_data: PathBuf) -> Result<Self> {
        let root = app_local_data;
        let wallpapers = root.join("wallpapers");
        let default_dir = root.join("default");
        std::fs::create_dir_all(&wallpapers)?;
        std::fs::create_dir_all(&default_dir)?;
        Ok(Self {
            db: root.join("wallflow.db"),
            wallpapers,
            default_dir,
        })
    }

    pub fn wallpaper_path(&self, file_name: &str) -> Result<PathBuf> {
        let safe = sanitize_file_name(file_name)?;
        Ok(self.wallpapers.join(safe))
    }
}

/// Reject anything that is not a bare, single-segment filename. This is what
/// stops a provider-supplied id from escaping the cache directory.
pub fn sanitize_file_name(name: &str) -> Result<String> {
    if name.is_empty() || name.len() > 128 {
        return Err(anyhow!("invalid file name length"));
    }
    if name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains(':')
        || name.starts_with('.')
    {
        return Err(anyhow!("unsafe file name: {name}"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(anyhow!("unsafe characters in file name: {name}"));
    }
    Ok(name.to_string())
}

/// Build a filename we control entirely, rather than trusting remote ids.
pub fn build_file_name(provider: &str, remote_id: &str) -> String {
    let clean: String = remote_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(48)
        .collect();
    let clean = if clean.is_empty() {
        format!("{}", chrono::Utc::now().timestamp())
    } else {
        clean
    };
    format!("{provider}-{clean}.jpg")
}

pub struct ValidatedImage {
    pub width: u32,
    pub height: u32,
    pub jpeg: Vec<u8>,
    pub format: &'static str,
}

/// Treat the payload strictly as image data: it is fully decoded before we will
/// touch it, so a malformed or disguised file is rejected rather than written.
///
/// A JPEG that decodes cleanly is then stored byte-for-byte as it arrived.
/// Re-encoding it would be a second lossy generation for no benefit — the decode
/// has already proved the bytes are a real image, which is the security point.
/// Only PNG is converted, because Windows handles JPEG wallpapers best.
pub fn validate_and_normalize(bytes: &[u8], min_width: u32, min_height: u32) -> Result<ValidatedImage> {
    if bytes.len() < MIN_IMAGE_BYTES {
        return Err(anyhow!("downloaded image is too small to be valid"));
    }
    let format = image::guess_format(bytes)
        .map_err(|_| anyhow!("downloaded data is not a recognised image"))?;
    if !matches!(format, image::ImageFormat::Jpeg | image::ImageFormat::Png) {
        return Err(anyhow!("unsupported image format: {format:?}"));
    }
    let decoded = image::load_from_memory_with_format(bytes, format)
        .map_err(|e| anyhow!("image failed to decode: {e}"))?;

    let (width, height) = (decoded.width(), decoded.height());
    if width < min_width || height < min_height {
        return Err(anyhow!(
            "image is {width}x{height}, below the {min_width}x{min_height} needed for this display"
        ));
    }

    let (jpeg, stored_format) = match format {
        image::ImageFormat::Jpeg => (bytes.to_vec(), "jpeg"),
        _ => {
            let rgb = decoded.to_rgb8();
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92).write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )?;
            (out, "jpeg (converted from png)")
        }
    };

    Ok(ValidatedImage {
        width,
        height,
        jpeg,
        format: stored_format,
    })
}

/// Write via a temp file + rename so a crash mid-write can never leave a
/// half-written file that later gets set as the wallpaper.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
