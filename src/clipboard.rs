//! Clipboard image retrieval.
//!
//! Primary path: [`arboard`] (cross-platform clipboard crate). On a pure
//! Wayland session where arboard cannot reach the clipboard, we fall back to
//! `wl-paste --type image/png` from the `wl-clipboard` package, which is what
//! danklinux's `dms screenshot` populates.

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use std::process::Command;

/// Read the current clipboard image.
///
/// Tries [`arboard`] first; on any failure falls back to `wl-paste`.
pub fn read_clipboard_image() -> Result<RgbaImage> {
    match try_arboard() {
        Ok(img) => Ok(img),
        Err(e) => {
            eprintln!("pinapp: arboard clipboard read failed ({e}); trying wl-paste");
            try_wl_paste().context("clipboard read via wl-paste also failed")
        }
    }
}

/// Read the clipboard image via [`arboard`].
fn try_arboard() -> Result<RgbaImage> {
    let mut clipboard = arboard::Clipboard::new().context("arboard::Clipboard::new")?;
    let data = clipboard.get_image().context("arboard get_image")?;
    let width = data.width as u32;
    let height = data.height as u32;
    // arboard always returns raw bytes in RGBA order.
    let bytes: Vec<u8> = data.bytes.to_vec();
    RgbaImage::from_raw(width, height, bytes)
        .context("RgbaImage::from_raw (arboard RGBA buffer)")
}

/// Read the clipboard image via `wl-paste --type image/png`.
fn try_wl_paste() -> Result<RgbaImage> {
    let output = Command::new("wl-paste")
        .arg("--type")
        .arg("image/png")
        .output()
        .context("spawn wl-paste (is wl-clipboard installed?)")?;
    if !output.status.success() {
        bail!("wl-paste exited with status {}", output.status);
    }
    let img = image::load_from_memory(&output.stdout)
        .context("decode png returned by wl-paste")?
        .to_rgba8();
    Ok(img)
}
