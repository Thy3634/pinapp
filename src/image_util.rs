//! Image utilities: cache directory management, (re)encoding to PNG, and the
//! pixel transforms used by [`crate::canvas::PinCanvas`] (rotate / flip /
//! opacity).

use anyhow::{Context, Result};
use image::{imageops, RgbaImage};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-process counter used to mint unique ids without extra dependencies.
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Return the pinapp cache directory (`$XDG_CACHE_HOME/pinapp`), creating it if
/// necessary. Falls back to `$HOME/.cache/pinapp` and then to `./.cache`.
pub fn cache_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|_| std::path::PathBuf::from(".cache"));
    let dir = base.join("pinapp");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Mint a process-unique id (nanos since epoch + counter).
pub fn gen_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}{counter:x}")
}

/// Build the cache file path for a given pin id and sequence number.
pub fn path_for(id: &str, seq: u32) -> std::path::PathBuf {
    cache_dir().join(format!("{id}_{seq}.png"))
}

/// Encode an [`RgbaImage`] as PNG to `path`.
pub fn save_png(img: &RgbaImage, path: &Path) -> Result<()> {
    image::DynamicImage::ImageRgba8(img.clone())
        .save(path)
        .with_context(|| format!("failed to save png to {path:?}"))?;
    Ok(())
}

/// Rotate an image 90 degrees clockwise.
pub fn rotate_cw(img: &RgbaImage) -> RgbaImage {
    imageops::rotate90(img)
}

/// Rotate an image 90 degrees counter-clockwise.
pub fn rotate_ccw(img: &RgbaImage) -> RgbaImage {
    imageops::rotate270(img)
}

/// Flip an image horizontally (left-right).
pub fn flip_horizontal(img: &RgbaImage) -> RgbaImage {
    imageops::flip_horizontal(img)
}

/// Flip an image vertically (top-bottom).
pub fn flip_vertical(img: &RgbaImage) -> RgbaImage {
    imageops::flip_vertical(img)
}

/// Multiply every pixel's alpha channel by `opacity` (clamped to [0, 1]).
pub fn apply_opacity(img: &RgbaImage, opacity: f32) -> RgbaImage {
    let factor = opacity.clamp(0.0, 1.0);
    let alpha = (factor * 255.0).round() as u32;
    let mut out = img.clone();
    for pixel in out.pixels_mut() {
        let current = pixel.0[3] as u32;
        pixel.0[3] = ((current * alpha) / 255) as u8;
    }
    out
}
