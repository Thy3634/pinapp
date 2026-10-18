//! PinView — a single pinned image window.
//!
//! ## Design note (Wayland / gpui-kit 0.7 limitation)
//!
//! gpui-kit 0.7 does **not** expose a runtime window-move/resize API
//! (`set_window_bounds` does not exist), so each pin is implemented as a
//! transparent, borderless, *screen-sized* floating window. The image is drawn
//! as an absolutely-positioned element centered inside that window. Wayland
//! click-through is achieved with [`Window::set_input_region`]:
//!
//!   * normal mode   → input region = the image rectangle only
//!     (everything outside the image passes clicks through to windows beneath)
//!   * click-through → input region = empty (the whole surface passes through,
//!     including the image itself)

use gpui_kit::prelude::*;
use gpui_kit::{
    App, AppContext, Bounds, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, Size, Window, div, img, px,
};
use image::RgbaImage;
use std::path::PathBuf;

use crate::image_util;

/// A single pinned image.
pub struct PinView {
    /// Original, untransformed image (never mutated).
    base: RgbaImage,
    /// Stable cache id used to build unique PNG file names.
    cache_id: String,
    /// Sequence counter for re-encoded content files.
    seq: u32,
    /// Path of the currently displayed (re-encoded) PNG.
    content_path: PathBuf,

    /// Display scale factor (0.1 .. 5.0).
    scale: f32,
    /// Opacity multiplier (0.2 .. 1.0).
    opacity: f32,
    /// Quarter-turn rotations clockwise (0..4).
    rot: u8,
    /// Horizontal flip.
    flip_h: bool,
    /// Vertical flip.
    flip_v: bool,
    /// Click-through mode (whole surface passes through).
    click_through: bool,

    /// Top-left position of the image within the window (in pixels).
    pos: Point<Pixels>,
    /// Whether a left-drag is in progress.
    dragging: bool,
    /// Last seen pointer position during a drag.
    drag_last: Point<Pixels>,

    /// Cache of the last applied input-region state (avoids redundant calls).
    applied_ct: bool,
    applied_bounds: Bounds<Pixels>,
}

impl PinView {
    /// Create a new pinned image, centered and scaled to fit <= 60% of the
    /// primary display.
    pub fn new(base: RgbaImage, cx: &App) -> Self {
        let id = image_util::gen_id();
        let screen = crate::daemon::primary_screen_bounds(cx);
        let (nw, nh) = (base.width() as f32, base.height() as f32);
        let max_dim = nw.max(nh).max(1.0);
        let screen_max = screen.size.width.0.max(screen.size.height.0);
        let scale = (0.6 * screen_max / max_dim).min(1.0).clamp(0.1, 5.0);
        let disp_w = nw * scale;
        let disp_h = nh * scale;
        let pos = Point {
            x: px((screen.size.width.0 - disp_w) / 2.0),
            y: px((screen.size.height.0 - disp_h) / 2.0),
        };

        let mut view = PinView {
            base,
            cache_id: id,
            seq: 0,
            content_path: PathBuf::new(),
            scale,
            opacity: 1.0,
            rot: 0,
            flip_h: false,
            flip_v: false,
            click_through: false,
            pos,
            dragging: false,
            drag_last: Point { x: px(0.0), y: px(0.0) },
            applied_ct: false,
            applied_bounds: Bounds {
                origin: Point { x: px(0.0), y: px(0.0) },
                size: Size {
                    width: px(0.0),
                    height: px(0.0),
                },
            },
        };
        view.reencode();
        view
    }

    /// Natural (post-rotation) pixel size of the image.
    fn natural_size(&self) -> (f32, f32) {
        let (w, h) = (self.base.width() as f32, self.base.height() as f32);
        if self.rot % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        }
    }

    fn current_w(&self) -> f32 {
        self.natural_size().0
    }

    fn current_h(&self) -> f32 {
        self.natural_size().1
    }

    /// Current on-screen rectangle of the image (in window coordinates).
    fn image_bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: self.pos,
            size: Size {
                width: px(self.current_w() * self.scale),
                height: px(self.current_h() * self.scale),
            },
        }
    }

    /// Re-encode the transformed image to a fresh cache file and point
    /// `content_path` at it. Only pixel-affecting transforms trigger this
    /// (rotation / flip / opacity); scale and position do not.
    fn reencode(&mut self) {
        let mut img = self.base.clone();
        for _ in 0..self.rot {
            img = image_util::rotate_cw(&img);
        }
        if self.flip_h {
            img = image_util::flip_horizontal(&img);
        }
        if self.flip_v {
            img = image_util::flip_vertical(&img);
        }
        if (self.opacity - 1.0).abs() > 1e-3 {
            img = image_util::apply_opacity(&img, self.opacity);
        }

        let new_path = image_util::path_for(&self.cache_id, self.seq);
        self.seq += 1;
        if self.content_path.exists() {
            let _ = std::fs::remove_file(&self.content_path);
        }
        let _ = image_util::save_png(&img, &new_path);
        self.content_path = new_path;
    }

    /// Apply the Wayland input region so that only the image (or nothing, in
    /// click-through mode) captures pointer input. Idempotent.
    fn sync_input_region(&mut self, window: &mut Window) {
        let b = self.image_bounds();
        let should_ct = self.click_through;
        let changed = should_ct != self.applied_ct || (!should_ct && self.applied_bounds != b);
        if !changed {
            return;
        }
        if should_ct {
            window.set_input_region(Some(&[]));
        } else {
            window.set_input_region(Some(&[b]));
        }
        self.applied_ct = should_ct;
        self.applied_bounds = b;
    }
}

impl Render for PinView {
    fn render(&mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Keep the Wayland input region in sync (must run before paint).
        self.sync_input_region(window);

        let w = px(self.current_w() * self.scale);
        let h = px(self.current_h() * self.scale);
        let left = self.pos.x;
        let top = self.pos.y;
        let path = self.content_path.clone();

        let image_el = img(path).absolute().left(left).top(top).w(w).h(h);

        div()
            .id("pin-root")
            .relative()
            .size_full()
            .child(image_el)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut Self, e: &MouseDownEvent, _window, _cx| {
                    this.dragging = true;
                    this.drag_last = e.position;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _e: &MouseUpEvent, _window, _cx| {
                    this.dragging = false;
                }),
            )
            .on_mouse_move(cx.listener(|this: &mut Self, e: &MouseMoveEvent, _window, cx| {
                if this.dragging {
                    let dx = e.position.x - this.drag_last.x;
                    let dy = e.position.y - this.drag_last.y;
                    this.pos.x += dx;
                    this.pos.y += dy;
                    this.drag_last = e.position;
                    cx.notify();
                }
            }))
            .on_scroll_wheel(cx.listener(|this: &mut Self, e: &ScrollWheelEvent, _window, cx| {
                let amount = match &e.delta {
                    ScrollDelta::Pixels(p) => p.y.0,
                    ScrollDelta::Lines(l) => l.y * 20.0,
                };
                if e.modifiers.control {
                    // Ctrl + wheel -> opacity
                    let delta = if amount > 0.0 { 0.05 } else { -0.05 };
                    this.opacity = (this.opacity + delta).clamp(0.2, 1.0);
                    this.reencode();
                } else {
                    // wheel -> scale
                    let factor = if amount > 0.0 { 1.05 } else { 1.0 / 1.05 };
                    this.scale = (this.scale * factor).clamp(0.1, 5.0);
                }
                cx.notify();
            }))
            .on_key_down(cx.listener(|this: &mut Self, e: &KeyDownEvent, window, cx| {
                let key = e.keystroke.key.as_str();
                match key {
                    "escape" => {
                        window.remove_window();
                    }
                    "r" => {
                        let steps = if e.keystroke.modifiers.shift { 3 } else { 1 };
                        this.rot = (this.rot + steps) % 4;
                        this.reencode();
                        cx.notify();
                    }
                    "f" => {
                        this.flip_h = !this.flip_h;
                        this.reencode();
                        cx.notify();
                    }
                    "v" => {
                        this.flip_v = !this.flip_v;
                        this.reencode();
                        cx.notify();
                    }
                    "t" => {
                        this.click_through = !this.click_through;
                        cx.notify();
                    }
                    _ => {}
                }
            }))
    }
}
