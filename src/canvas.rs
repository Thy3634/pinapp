//! The canvas view: one transparent, borderless, screen-sized window that
//! hosts **every** pinned image as a child element.
//!
//! ## Why a single canvas instead of one window per pin
//!
//! gpui-kit 0.7 / gpui-pre 0.3.9 exposes no runtime window-move/resize API
//! (`set_window_bounds` does not exist), and driving each image through its own
//! full-screen window makes stacking and moving images needlessly awkward.
//! Instead one window spans the whole screen and is drawn transparently, while
//! each image is an absolutely positioned child.
//!
//! Wayland click-through is expressed purely with `window.set_input_region`
//! (see [`gpui_kit::Window::set_input_region`], documented as Linux/Wayland
//! only):
//!
//! | State                         | Region                                  |
//! | ----------------------------- | --------------------------------------- |
//! | normal                        | union of the non-click-through rects    |
//! | dragging                      | `None` → the whole window is active     |
//! | no pins / all click-through   | empty region → whole window passes      |
//!
//! ## DDD layering
//!
//! This module is pure presentation. All image state lives in the framework
//! free domain model [`crate::pinned_image::PinnedImage`]; this file only owns
//! window geometry, focus, shadowing and the input region.

use gpui_kit::prelude::*;
use gpui_kit::{
    Bounds, BoxShadow, Context, Hsla, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, Size,
    Window, div, hsla, img, point, px, rgb,
};
use image::RgbaImage;

use crate::pinned_image::PinnedImage;

/// State of one pinned image inside the canvas.
struct PinItem {
    /// Stable identity; event closures capture this so reordering the vector
    /// can never invalidate a live closure.
    id: u64,
    /// The domain model holding the pixels + transform state.
    image: PinnedImage,
    /// Top-left corner of the image, in window coordinates.
    pos: Point<Pixels>,
    /// Whether pointer input passes straight through this image.
    click_through: bool,
    /// Whether the image is currently being dragged.
    dragging: bool,
    /// Last observed pointer position while dragging.
    drag_last: Point<Pixels>,
}

impl PinItem {
    /// Rectangle this image occupies on screen.
    fn rect(&self) -> Bounds<Pixels> {
        let (w, h) = self.image.display_size();
        Bounds {
            origin: self.pos,
            size: Size {
                width: px(w),
                height: px(h),
            },
        }
    }
}

/// The transparent, screen-sized window that hosts every pinned image.
pub struct PinCanvas {
    /// Pins in **render order**, which doubles as the z-order: later = on top.
    pins: Vec<PinItem>,
    /// Id of the focused pin, when at least one pin exists.
    focused: Option<u64>,
    /// Accent color used for the focused pin's glow.
    accent: Hsla,
    /// Monotonic id source.
    next_id: u64,
    /// The input region last pushed to the platform, if any.
    applied_region: Option<Vec<Bounds<Pixels>>>,
    /// Whether any region has been pushed at least once.
    region_inited: bool,
    /// Whether any pin is mid-drag (widens the input region).
    dragging_any: bool,
}

impl PinCanvas {
    /// Create an empty canvas. No `&App` is required: the first `add_pin`
    /// receives the screen size from the caller, which already holds one.
    pub fn new() -> Self {
        // The application accent (bright blue); the theme window can later
        // make this configurable.
        let accent: Hsla = rgb(0x4f8cff).into();
        PinCanvas {
            pins: Vec::new(),
            focused: None,
            accent,
            next_id: 0,
            applied_region: None,
            region_inited: false,
            dragging_any: false,
        }
    }

    /// Add a pinned image, centered on `screen`, and focus it (so it lands on
    /// top of the stack).
    ///
    /// Returns the id of the newly added pin.
    pub fn add_pin(&mut self, img: RgbaImage, screen: Bounds<Pixels>) -> u64 {
        let image = PinnedImage::new(img, screen.size.width.as_f32(), screen.size.height.as_f32());
        let (w, h) = image.display_size();
        let pos = point(
            px((screen.size.width.as_f32() - w) / 2.0),
            px((screen.size.height.as_f32() - h) / 2.0),
        );
        let id = self.next_id;
        self.next_id += 1;
        // Pushing at the end also puts it on top; focusing it is implied.
        self.pins.push(PinItem {
            id,
            image,
            pos,
            click_through: false,
            dragging: false,
            drag_last: point(px(0.0), px(0.0)),
        });
        self.focused = Some(id);
        id
    }

    /// Bring the pin with `id` to the top of the stack and focus it.
    ///
    /// Focusing by id (rather than by index) keeps the callbacks immune to
    /// reordering caused by another pin being raised.
    fn focus_by_id(&mut self, id: u64) {
        if let Some(index) = self.pins.iter().position(|p| p.id == id) {
            let pin = self.pins.remove(index);
            self.pins.push(pin);
        }
        self.focused = Some(id);
    }

    /// Mutable access to the focused pin.
    fn focused_mut(&mut self) -> Option<&mut PinItem> {
        let id = self.focused?;
        self.pins.iter_mut().find(|p| p.id == id)
    }

    /// Push the desired input region to the window, but only when it actually
    /// changes (the region call crosses to the platform each time).
    fn sync_input_region(&mut self, window: &mut Window) {
        let target: Option<Vec<Bounds<Pixels>>> = if self.dragging_any {
            // Whole window, so a fast pointer cannot leave the dragged image.
            None
        } else if self.pins.is_empty() {
            // No pins at all (or all click-through): pass everything through.
            Some(Vec::new())
        } else {
            Some(
                self.pins
                    .iter()
                    .filter(|p| !p.click_through)
                    .map(PinItem::rect)
                    .collect(),
            )
        };

        let changed = !self.region_inited || self.applied_region != target;
        if !changed {
            return;
        }
        window.set_input_region(target.as_deref());
        self.applied_region = target;
        self.region_inited = true;
    }
}

impl Render for PinCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        // Keep the Wayland input region in sync before anything is painted.
        self.sync_input_region(window);

        // Values read out up front so the loop below only touches `self.pins`
        // immutably while registering listeners through `cx`.
        let accent = self.accent;
        let focused = self.focused;

        // Child elements in z-order; the last one paints on top.
        let mut children = Vec::with_capacity(self.pins.len());
        for pin in self.pins.iter() {
            let id = pin.id;
            let pos = pin.pos;
            let (w, h) = pin.image.display_size();
            let path = pin.image.content_path().clone();
            let is_focused = Some(id) == focused;

            // A focused pin gets the accent glow; the rest a neutral shadow.
            let shadow = if is_focused {
                BoxShadow::new(px(0.0), px(8.0), accent)
                    .blur_radius(px(24.0))
                    .spread_radius(px(2.0))
            } else {
                BoxShadow::new(px(0.0), px(4.0), hsla(0.0, 0.0, 0.0, 0.45)).blur_radius(px(14.0))
            };

            let element = img(path)
                .absolute()
                .left(pos.x)
                .top(pos.y)
                .w(px(w))
                .h(px(h))
                .shadow(vec![shadow])
                // Focus + begin dragging.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Self, e: &MouseDownEvent, _window, cx| {
                        this.focus_by_id(id);
                        if let Some(pin) = this.pins.iter_mut().find(|p| p.id == id) {
                            pin.dragging = true;
                            pin.drag_last = e.position;
                        }
                        this.dragging_any = true;
                        cx.notify();
                    }),
                )
                // End dragging; recompute the global drag flag.
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Self, _e: &MouseUpEvent, _window, cx| {
                        if let Some(pin) = this.pins.iter_mut().find(|p| p.id == id) {
                            pin.dragging = false;
                        }
                        this.dragging_any = this.pins.iter().any(|p| p.dragging);
                        cx.notify();
                    }),
                )
                // Translate the dragged pin by the pointer delta.
                .on_mouse_move(cx.listener(move |this: &mut Self, e: &MouseMoveEvent, _window, cx| {
                    if let Some(pin) = this.pins.iter_mut().find(|p| p.id == id) {
                        if pin.dragging {
                            let dx = e.position.x.as_f32() - pin.drag_last.x.as_f32();
                            let dy = e.position.y.as_f32() - pin.drag_last.y.as_f32();
                            pin.pos.x = px(pin.pos.x.as_f32() + dx);
                            pin.pos.y = px(pin.pos.y.as_f32() + dy);
                            pin.drag_last = e.position;
                            cx.notify();
                        }
                    }
                }))
                // Ctrl + wheel adjusts opacity, plain wheel adjusts scale. Both
                // act on the focused pin.
                .on_scroll_wheel(cx.listener(move |this: &mut Self, e: &ScrollWheelEvent, _window, cx| {
                    let delta = match &e.delta {
                        ScrollDelta::Pixels(p) => p.y.as_f32(),
                        ScrollDelta::Lines(l) => l.y * 20.0,
                    };
                    if e.modifiers.secondary() {
                        // Scroll up (negative delta) means "more opaque".
                        let step = if delta < 0.0 { 0.05 } else { -0.05 };
                        if let Some(pin) = this.focused_mut() {
                            pin.image.transform_mut().adjust_opacity(step);
                            pin.image.reencode();
                        }
                    } else {
                        // Scroll up means "zoom in".
                        let factor = if delta < 0.0 { 1.1 } else { 1.0 / 1.1 };
                        if let Some(pin) = this.focused_mut() {
                            pin.image.transform_mut().adjust_scale(factor);
                        }
                    }
                    cx.notify();
                }));

            children.push(element);
        }

        div()
            .id("pin-canvas")
            .relative()
            .size_full()
            .children(children)
            .on_key_down(cx.listener(|this: &mut Self, e: &KeyDownEvent, _window, cx| {
                let key = e.keystroke.key.to_lowercase();
                let shift = e.keystroke.modifiers.shift;
                match key.as_str() {
                    // Close the focused pin; the canvas window stays alive.
                    "escape" => {
                        if let Some(id) = this.focused {
                            if let Some(index) = this.pins.iter().position(|p| p.id == id) {
                                this.pins.remove(index);
                            }
                            // Move focus to the new top-most pin, if any.
                            this.focused = this.pins.last().map(|p| p.id);
                            cx.notify();
                        }
                    }
                    // Rotate; Shift reverses the direction.
                    "r" => {
                        let steps = if shift { 3 } else { 1 };
                        if let Some(pin) = this.focused_mut() {
                            pin.image.transform_mut().rotate(steps);
                            pin.image.reencode();
                        }
                        cx.notify();
                    }
                    // Horizontal / vertical flip.
                    "f" => {
                        if let Some(pin) = this.focused_mut() {
                            pin.image.transform_mut().toggle_flip_h();
                            pin.image.reencode();
                        }
                        cx.notify();
                    }
                    "v" => {
                        if let Some(pin) = this.focused_mut() {
                            pin.image.transform_mut().toggle_flip_v();
                            pin.image.reencode();
                        }
                        cx.notify();
                    }
                    // Toggle click-through for the focused pin.
                    "t" => {
                        if let Some(pin) = this.focused_mut() {
                            pin.click_through = !pin.click_through;
                        }
                        cx.notify();
                    }
                    _ => {}
                }
            }))
    }
}
