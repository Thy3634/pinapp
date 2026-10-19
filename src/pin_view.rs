//! PinView — the presentation layer for a single pinned image window.
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
//!
//! The image *content* and its transforms live in the framework-free domain
//! model [`crate::pinned_image::PinnedImage`]; this struct only owns window
//! position, drag state, and the click-through flag.

use gpui_kit::prelude::*;
use gpui_kit::{
    App, AppContext, Bounds, Context, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, Size, Window, div, img, px,
};
use image::RgbaImage;

use crate::pinned_image::PinnedImage;

/// A single pinned image window (presentation layer).
pub struct PinView {
    /// The framework-free domain model holding the image + transforms.
    pinned: PinnedImage,

    /// Top-left position of the image within the window (in pixels).
    pos: Point<Pixels>,
    /// Whether a left-drag is in progress.
    dragging: bool,
    /// Last seen pointer position during a drag.
    drag_last: Point<Pixels>,

    /// Click-through mode (whole surface passes through).
    click_through: bool,
    /// Cache of the last applied input-region state (avoids redundant calls).
    applied_ct: bool,
    applied_bounds: Bounds<Pixels>,
}

impl PinView {
    /// Create a new pinned image, centered and scaled to fit <= 60% of the
    /// primary display.
    pub fn new(img: RgbaImage, cx: &App) -> Self {
        let screen = crate::daemon::primary_screen_bounds(cx);
        let pinned = PinnedImage::new(img, screen.size.width.0, screen.size.height.0);
        let (dw, dh) = pinned.display_size();
        let pos = Point {
            x: px((screen.size.width.0 - dw) / 2.0),
            y: px((screen.size.height.0 - dh) / 2.0),
        };

        PinView {
            pinned,
            pos,
            dragging: false,
            drag_last: Point { x: px(0.0), y: px(0.0) },
            click_through: false,
            applied_ct: false,
            applied_bounds: Bounds {
                origin: Point { x: px(0.0), y: px(0.0) },
                size: Size {
                    width: px(0.0),
                    height: px(0.0),
                },
            },
        }
    }

    /// Current on-screen rectangle of the image (in window coordinates).
    fn image_bounds(&self) -> Bounds<Pixels> {
        let (w, h) = self.pinned.display_size();
        Bounds {
            origin: self.pos,
            size: Size {
                width: px(w),
                height: px(h),
            },
        }
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        // Keep the Wayland input region in sync (must run before paint).
        self.sync_input_region(window);

        let (w, h) = self.pinned.display_size();
        let path = self.pinned.content_path().clone();

        let image_el = img(path).absolute().left(self.pos.x).top(self.pos.y).w(px(w)).h(px(h));

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
                    this.pinned.transform_mut().adjust_opacity(delta);
                    this.pinned.reencode();
                } else {
                    // wheel -> scale
                    let factor = if amount > 0.0 { 1.05 } else { 1.0 / 1.05 };
                    this.pinned.transform_mut().adjust_scale(factor);
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
                        this.pinned.transform_mut().rotate(steps);
                        this.pinned.reencode();
                        cx.notify();
                    }
                    "f" => {
                        this.pinned.transform_mut().toggle_flip_h();
                        this.pinned.reencode();
                        cx.notify();
                    }
                    "v" => {
                        this.pinned.transform_mut().toggle_flip_v();
                        this.pinned.reencode();
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
