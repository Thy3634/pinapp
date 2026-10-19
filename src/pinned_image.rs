//! Domain model: a pinned image and its on-screen transforms.
//!
//! This module is intentionally **free of any `gpui_kit` / GPUI dependency** so
//! the core pinning logic (transform state + (re)encoding pipeline) is a pure,
//! unit-testable domain model — per the DDD red line that the domain layer must
//! not depend on the presentation framework.
//!
//! The presentation layer ([`crate::pin_view`]) owns window position, drag
//! state and the Wayland input region; it delegates all image transforms to
//! this model.

use image::RgbaImage;
use std::path::PathBuf;

use crate::image_util;

/// Transform state of a pinned image — the domain's only mutable state.
///
/// Pure value object: no framework, no I/O. All mutations are clamped to the
/// ranges the UI supports.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    /// Display scale factor (0.1 .. 5.0).
    pub scale: f32,
    /// Opacity multiplier (0.2 .. 1.0).
    pub opacity: f32,
    /// Quarter-turn rotations clockwise (0..4).
    pub rot: u8,
    /// Horizontal flip.
    pub flip_h: bool,
    /// Vertical flip.
    pub flip_v: bool,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            scale: 1.0,
            opacity: 1.0,
            rot: 0,
            flip_h: false,
            flip_v: false,
        }
    }
}

impl Transform {
    /// Multiply the scale by `factor`, clamped to [0.1, 5.0].
    pub fn adjust_scale(&mut self, factor: f32) {
        self.scale = (self.scale * factor).clamp(0.1, 5.0);
    }

    /// Add `delta` to opacity, clamped to [0.2, 1.0].
    pub fn adjust_opacity(&mut self, delta: f32) {
        self.opacity = (self.opacity + delta).clamp(0.2, 1.0);
    }

    /// Rotate by `steps` quarter-turns clockwise (mod 4).
    pub fn rotate(&mut self, steps: u8) {
        self.rot = (self.rot + steps) % 4;
    }

    pub fn toggle_flip_h(&mut self) {
        self.flip_h = !self.flip_h;
    }

    pub fn toggle_flip_v(&mut self) {
        self.flip_v = !self.flip_v;
    }

    /// Natural (post-rotation) pixel size of the image.
    pub fn natural_size(&self, base: &(u32, u32)) -> (f32, f32) {
        let (w, h) = (base.0 as f32, base.1 as f32);
        if self.rot % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        }
    }
}

/// A pinned image: the original pixels plus the transform applied to them, with
/// a re-encoded PNG cache the presentation layer can load.
pub struct PinnedImage {
    /// Original, untransformed image (never mutated).
    base: RgbaImage,
    /// (width, height) of `base`, cached to avoid recomputing.
    base_size: (u32, u32),
    /// Stable cache id used to build unique PNG file names.
    cache_id: String,
    /// Sequence counter for re-encoded content files.
    seq: u32,
    /// Path of the currently displayed (re-encoded) PNG.
    content_path: PathBuf,
    /// Applied transform.
    transform: Transform,
}

impl PinnedImage {
    /// Create a new pinned image, scaled to fit <= 60% of the given screen
    /// dimensions (in pixels). The screen size is passed as plain values rather
    /// than a framework object to keep this model framework-free.
    pub fn new(base: RgbaImage, screen_w: f32, screen_h: f32) -> Self {
        let (nw, nh) = (base.width() as f32, base.height() as f32);
        let max_dim = nw.max(nh).max(1.0);
        let screen_max = screen_w.max(screen_h);
        let scale = (0.6 * screen_max / max_dim).min(1.0).clamp(0.1, 5.0);

        let mut pinned = PinnedImage {
            base_size: (base.width(), base.height()),
            base,
            cache_id: image_util::gen_id(),
            seq: 0,
            content_path: PathBuf::new(),
            transform: Transform {
                scale,
                ..Default::default()
            },
        };
        pinned.reencode();
        pinned
    }

    /// Mutable access to the transform (UI calls this then [`Self::reencode`]).
    pub fn transform_mut(&mut self) -> &mut Transform {
        &mut self.transform
    }

    /// Read-only access to the transform.
    pub fn transform(&self) -> &Transform {
        &self.transform
    }

    /// Path of the current re-encoded PNG (what the view should display).
    pub fn content_path(&self) -> &PathBuf {
        &self.content_path
    }

    /// On-screen size (post-rotation, post-scale) in pixels.
    pub fn display_size(&self) -> (f32, f32) {
        let (w, h) = self.transform.natural_size(&self.base_size);
        (w * self.transform.scale, h * self.transform.scale)
    }

    /// Re-encode the transformed image to a fresh cache file and point
    /// `content_path` at it. Only pixel-affecting transforms are applied
    /// (rotation / flip / opacity); scale is a display-only concern.
    pub fn reencode(&mut self) {
        let mut img = self.base.clone();
        for _ in 0..self.transform.rot {
            img = image_util::rotate_cw(&img);
        }
        if self.transform.flip_h {
            img = image_util::flip_horizontal(&img);
        }
        if self.transform.flip_v {
            img = image_util::flip_vertical(&img);
        }
        if (self.transform.opacity - 1.0).abs() > 1e-3 {
            img = image_util::apply_opacity(&img, self.transform.opacity);
        }

        let new_path = image_util::path_for(&self.cache_id, self.seq);
        self.seq += 1;
        if self.content_path.exists() {
            let _ = std::fs::remove_file(&self.content_path);
        }
        let _ = image_util::save_png(&img, &new_path);
        self.content_path = new_path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_image(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba([255u8, 0, 0, 255]))
    }

    #[test]
    fn transform_scale_clamps() {
        let mut t = Transform::default();
        for _ in 0..100 {
            t.adjust_scale(1.05);
        }
        assert!((t.scale - 5.0).abs() < 1e-3, "scale should clamp to 5.0");
        for _ in 0..200 {
            t.adjust_scale(1.0 / 1.05);
        }
        assert!((t.scale - 0.1).abs() < 1e-3, "scale should clamp to 0.1");
    }

    #[test]
    fn transform_opacity_clamps() {
        let mut t = Transform::default();
        t.adjust_opacity(-1.0);
        assert!((t.opacity - 0.2).abs() < 1e-3);
        t.adjust_opacity(1.0);
        assert!((t.opacity - 1.0).abs() < 1e-3);
    }

    #[test]
    fn rotate_quarter_turns_mod4() {
        let mut t = Transform::default();
        t.rotate(1);
        assert_eq!(t.rot, 1);
        t.rotate(3);
        assert_eq!(t.rot, 0);
    }

    #[test]
    fn natural_size_swaps_on_odd_rotation() {
        let rotated = Transform {
            rot: 1,
            ..Default::default()
        };
        assert_eq!(rotated.natural_size(&(100, 200)), (200.0, 100.0));
        let upright = Transform {
            rot: 0,
            ..Default::default()
        };
        assert_eq!(upright.natural_size(&(100, 200)), (100.0, 200.0));
    }

    #[test]
    fn reencode_produces_valid_rotated_png() {
        let mut p = PinnedImage::new(solid_image(40, 30), 1920.0, 1080.0);
        p.transform_mut().rotate(1);
        p.reencode();
        assert!(p.content_path().exists(), "cache png should exist");
        let loaded = image::open(p.content_path()).expect("should decode png");
        // Rotated 90deg clockwise: width and height are swapped.
        assert_eq!(loaded.width(), 30);
        assert_eq!(loaded.height(), 40);
    }

    #[test]
    fn new_scales_down_large_image() {
        // A 4000x3000 image on a 1920x1080 screen should be scaled <= 1.0.
        let p = PinnedImage::new(solid_image(4000, 3000), 1920.0, 1080.0);
        assert!(p.transform().scale <= 1.0);
        let (w, h) = p.display_size();
        assert!(w.max(h) <= 0.6 * 1920.0 + 1e-3);
    }
}
