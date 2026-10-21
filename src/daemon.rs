//! Daemon: owns the GPUI application, serves IPC commands, and hosts the single
//! pin canvas.
//!
//! Architecture:
//!   * a dedicated OS thread accepts Unix-socket connections, reads a single
//!     command line, and forwards `(command, reply_sender)` into an
//!     [`async_channel`] [`Receiver`];
//!   * the GPUI app spawns a background task that `recv`s those commands and,
//!     on the main thread, reads the clipboard and adds the image to the single
//!     canvas window (GPUI windows may only be created from the main thread via
//!     an `update` call);
//!   * after processing a command, the main thread sends the outcome string
//!     ("ok" or "err:<message>") back through the reply channel; the listener
//!     thread writes it to the client as an acknowledgement.
//!
//! The canvas is held as a [`WindowHandle`](gpui_kit::WindowHandle) behind an
//! `Arc<Mutex<..>>` so it can be created once and reused for every subsequent
//! `pin`. The handle (unlike a bare `Entity`) lets us detect that the window was
//! closed underneath us via a `Result`, instead of panicking on a stale view.

use anyhow::Result;
use async_channel::unbounded;
use gpui_kit::{
    App, AppContext, AsyncApp, Bounds, PlatformDisplay, Point, Pixels, QuitMode, Size, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, px,
};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::canvas::PinCanvas;
use crate::clipboard;
use crate::ipc;

/// Run the persistent daemon. Blocks until the application quits.
pub fn run_daemon() -> Result<()> {
    let (tx, rx) = unbounded::<(String, mpsc::Sender<String>)>();

    // OS thread that accepts IPC connections and forwards commands + reply
    // channels.
    start_socket_listener(tx)?;

    // The one canvas window that hosts every pinned image. Shared across
    // commands; only ever touched from the GPUI main thread.
    let canvas: Arc<Mutex<Option<WindowHandle<PinCanvas>>>> = Arc::new(Mutex::new(None));

    let _ = gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);

        // The daemon must outlive the canvas window. With the default
        // `QuitMode::Default`, GPUI quits as soon as the last window goes away
        // (`cfg!(not(target_os = "macos"))` is true on Linux), and that happens
        // whenever the compositor dismisses our surface or the canvas is
        // closed. The daemon would then be gone and every following `pin` would
        // fail with ConnectionRefused. `QuitMode::Explicit` means the process
        // only ends via our own `cx.quit()`, i.e. the `quit` IPC command.
        cx.set_quit_mode(QuitMode::Explicit);

        let canvas = canvas.clone();
        // Background task: process IPC commands on the main thread so we can
        // create windows and touch entities safely.
        cx.spawn(async move |async_cx: &mut AsyncApp| {
            while let Ok((cmd, reply)) = rx.recv().await {
                match cmd.as_str() {
                    "pin" => match clipboard::read_clipboard_image() {
                        Ok(img) => {
                            let ack = async_cx.update(|cx| add_pin_to_canvas(&canvas, img, cx));
                            let _ = reply.send(ack);
                        }
                        Err(e) => {
                            eprintln!("pinapp: failed to read clipboard: {e}");
                            let _ = reply.send(format!("err:{e}"));
                        }
                    },
                    "quit" => {
                        let _ = reply.send("ok".to_string());
                        let _ = async_cx.update(|cx| cx.quit());
                        break;
                    }
                    other => {
                        eprintln!("pinapp: unknown command `{other}`");
                        let _ = reply.send(format!("err:unknown-command:{other}"));
                    }
                }
            }
        })
        .detach();
    });

    Ok(())
}

/// Add `img` to the shared canvas window, creating the window on first use (or
/// recreating it if it was closed).
///
/// Returns the acknowledgement string to send back to the client. Must be
/// called on the GPUI main thread (i.e. from inside `AsyncApp::update`).
fn add_pin_to_canvas(canvas: &Arc<Mutex<Option<WindowHandle<PinCanvas>>>>, img: image::RgbaImage, cx: &mut App) -> String {
    let screen = primary_screen_bounds(cx);

    // Probe whether the existing canvas window is still alive. The probe
    // closure does not move `img`, so it can be reused below on either path.
    let existing = canvas.lock().unwrap().clone();
    let alive = matches!(&existing, Some(handle) if handle.update(cx, |_: &mut PinCanvas, _, _| ()).is_ok());

    let handle = if alive {
        existing.unwrap()
    } else {
        // Either nothing was created yet, or the window was closed; recreate it.
        let _ = canvas.lock().unwrap().take();
        // NOTE: we deliberately call `App::open_window` (the inherent method)
        // instead of the `gpui_kit::open_window` free function. The latter wraps
        // the view in `gpui_base::Root` and returns `(AnyWindowHandle, Entity<V>)`,
        // which would make the stored `WindowHandle<PinCanvas>` downcast fail at
        // runtime ("the type of the window's root view has changed"). Using the
        // raw API keeps `PinCanvas` itself as the window's root view, which is
        // exactly what `WindowHandle<PinCanvas>::update` needs.
        let options = build_window_options(cx);
        match cx.open_window(options, |_, cx| cx.new(|_| PinCanvas::new())) {
            Ok(handle) => {
                *canvas.lock().unwrap() = Some(handle);
                handle
            }
            Err(e) => return format!("err:{e}"),
        }
    };

    match handle.update(cx, |view: &mut PinCanvas, _window, cx| {
        view.add_pin(img, screen);
        cx.notify();
    }) {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("err:{e}"),
    }
}

/// Bind the Unix socket and spawn a thread that forwards each received command
/// line as `(command, reply_sender)`. A stale socket file is removed first so a
/// crash doesn't block restart. After forwarding, the thread blocks on the
/// reply channel and writes the outcome back to the client as an ack.
fn start_socket_listener(
    tx: async_channel::Sender<(String, mpsc::Sender<String>)>,
) -> Result<()> {
    let path = ipc::socket_path();
    // Remove a stale socket left by a previous run.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .map_err(|e| anyhow::anyhow!("failed to bind Unix socket {path:?}: {e}"))?;

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => break,
            };

            // Read a single command line (until '\n' or EOF). We must NOT read
            // until EOF, otherwise we'd block until the client closes and could
            // never send an acknowledgement back.
            let mut line = String::new();
            let mut buf = [0u8; 256];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let chunk = &buf[..n];
                        if let Some(pos) = chunk.iter().position(|&b| b == b'\n') {
                            line.push_str(&String::from_utf8_lossy(&chunk[..pos]));
                            break;
                        } else {
                            line.push_str(&String::from_utf8_lossy(chunk));
                        }
                    }
                    Err(_) => break,
                }
            }
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            // Forward the command together with a one-shot reply channel.
            let (reply_tx, reply_rx) = mpsc::channel();
            if tx.try_send((line, reply_tx)).is_err() {
                return; // daemon shutting down
            }

            // Wait for the main thread to process the command, then write the
            // outcome back as the acknowledgement and close the connection.
            let ack = reply_rx
                .recv()
                .unwrap_or_else(|_| "err:daemon-shutdown".to_string());
            let _ = stream.write_all(format!("{ack}\n").as_bytes());
            let _ = stream.flush();
        }
    });
    Ok(())
}

/// Pick the display the pin canvas should cover.
///
/// `App::primary_display()` cannot be used here: on Wayland gpui-pre-linux
/// implements it as an unconditional `None`, so it always falls back. We
/// enumerate `App::displays()` instead and prefer the output whose bounds
/// contain the global origin, which is the usual primary output. When no output
/// starts at the origin (some multi-monitor layouts) we just take the first one.
///
/// Returns `None` only when the platform reports no displays at all.
fn target_display(cx: &App) -> Option<Rc<dyn PlatformDisplay>> {
    let displays = cx.displays();
    let index = displays
        .iter()
        .position(|d| d.bounds().contains(&Point {
            x: px(0.0),
            y: px(0.0),
        }))
        .unwrap_or(0);
    displays.into_iter().nth(index)
}

/// Compute the bounds of the display the canvas covers (fallback 1920x1080).
///
/// Uses the real display geometry when the platform reports any, so a pinned
/// image is centred on the actual viewport instead of a hardcoded size.
pub fn primary_screen_bounds(cx: &App) -> Bounds<Pixels> {
    target_display(cx)
        .map(|d| d.bounds())
        .unwrap_or_else(|| Bounds {
            origin: Point { x: px(0.0), y: px(0.0) },
            size: Size {
                width: px(1920.0),
                height: px(1080.0),
            },
        })
}

/// Build the [`WindowOptions`] for the canvas: transparent, borderless,
/// screen-sized, floating, with `app-id = "pinapp"`.
///
/// Note: gpui-kit 0.7 exposes no runtime window-move/resize API, so the canvas
/// is a full-screen transparent window and each image is positioned inside it
/// (see [`crate::canvas`]).
///
/// ## Why this is [`WindowKind::Floating`] and not [`WindowKind::LayerShell`]
///
/// `LayerShell` was tried first (commit 70d7431) and had to be reverted: niri
/// does not advertise a single `wl_output` global, only `zxdg_output_manager_v1`
/// and `zwlr_output_manager_v1`. gpui-pre-linux discovers displays exclusively
/// by binding `wl_output` globals, so `App::displays()` returns an empty list,
/// there is no `DisplayId` to pass, and `get_layer_surface` is called with a
/// NULL `wl_output`. niri answers by sending `closed()` immediately, before any
/// `configure`, so the canvas never appears at all. Passing an output is not
/// possible from here, so the layer-shell route is blocked on the platform
/// layer, not on this file.
///
/// `Floating` is what actually renders under niri. It is an ordinary toplevel
/// window, so the compositor gives it window semantics; to make it span the
/// whole viewport we ask for the fullscreen state rather than for a size, see
/// the note on `window_bounds` below.
fn build_window_options(cx: &App) -> WindowOptions {
    // Resolve the real output. Passing `display_id` matters for layer-shell:
    // with `None` GPUI hands a NULL `wl_output` to `get_layer_surface`, and
    // niri answers by closing the surface immediately (no `configure` is ever
    // sent), so the canvas never appears.
    let display = target_display(cx);
    let screen = display
        .as_ref()
        .map(|d| d.bounds())
        .unwrap_or_else(|| primary_screen_bounds(cx));
    let bounds = Bounds {
        origin: Point { x: px(0.0), y: px(0.0) },
        size: screen.size,
    };

    WindowOptions {
        // Ask the compositor for the fullscreen state instead of handing it a
        // size. niri advertises no `wl_output` global, so gpui-pre-linux finds
        // no displays at all and `primary_screen_bounds` can only ever return
        // its hardcoded fallback; a client-supplied size therefore cannot cover
        // the real viewport (niri tiled the canvas to 1920x1390 on a 2560-wide
        // output). `Fullscreen` makes GPUI request the fullscreen state via
        // `toggle_fullscreen`, so the compositor decides the covering size and
        // we do not need to know the real resolution. `bounds` is only the
        // restore size used after leaving fullscreen.
        window_bounds: Some(WindowBounds::Fullscreen(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("pinapp".into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: WindowKind::Floating,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        app_owns_titlebar_drag: false,
        inactive_frame_interval: None,
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some("pinapp".to_string()),
        // `None` => no (server/client) decorations → borderless on Linux.
        window_decorations: None,
        window_min_size: None,
        // Real output id so `get_layer_surface` receives a non-NULL wl_output.
        display_id: display.as_ref().map(|d| d.id()),
        icon: None,
        tabbing_identifier: None,
    }
}
