//! Daemon: owns the GPUI application, serves IPC commands, and opens pin
//! windows on demand.
//!
//! Architecture:
//!   * a dedicated OS thread accepts Unix-socket connections and forwards each
//!     received text line into an [`async_channel`] [`Receiver`];
//!   * the GPUI app spawns a background task that `recv`s those commands and,
//!     on the main thread, reads the clipboard and opens pin windows (GPUI
//!     windows may only be created from the main thread / an `update` call).

use anyhow::Result;
use async_channel::unbounded;
use gpui_kit::{
    App, AppContext, Bounds, PlatformDisplay, Point, Pixels, Size, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use crate::clipboard;
use crate::ipc;
use crate::pin_view::PinView;

/// Run the persistent daemon. Blocks until the application quits.
pub fn run_daemon() -> Result<()> {
    let (tx, rx) = unbounded::<String>();

    // OS thread that accepts IPC connections and forwards commands.
    start_socket_listener(tx)?;

    let _ = gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);

        // Background task: process IPC commands on the main thread so we can
        // open GPUI windows safely.
        let rx = rx;
        cx.spawn(|mut async_cx| async move {
            while let Ok(cmd) = rx.recv().await {
                match cmd.as_str() {
                    "pin" => match clipboard::read_clipboard_image() {
                        Ok(img) => {
                            let _ = async_cx.update(|cx| {
                                let opts = build_window_options(cx);
                                let _ = gpui_kit::open_window(opts, cx, |_, cx| {
                                    cx.new(|cx| PinView::new(img, cx))
                                });
                            });
                        }
                        Err(e) => eprintln!("pinapp: failed to read clipboard: {e}"),
                    },
                    "quit" => {
                        let _ = async_cx.update(|cx| cx.quit());
                        break;
                    }
                    other => eprintln!("pinapp: unknown command `{other}`"),
                }
            }
        })
        .detach();
    });

    Ok(())
}

/// Bind the Unix socket and spawn a thread that forwards each received line as a
/// command. A stale socket file is removed first so a crash doesn't block
/// restart.
fn start_socket_listener(tx: async_channel::Sender<String>) -> Result<()> {
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
            let mut buf = String::new();
            use std::io::Read;
            if stream.read_to_string(&mut buf).is_ok() {
                for line in buf.lines() {
                    let line = line.trim().to_string();
                    if !line.is_empty() {
                        if tx.send(line).is_err() {
                            // Receiver is gone (daemon shutting down).
                            return;
                        }
                    }
                }
            }
        }
    });
    Ok(())
}

/// Compute the bounds of the primary display (fallback 1920x1080).
pub fn primary_screen_bounds(cx: &App) -> Bounds<Pixels> {
    cx.primary_display()
        .map(|d| d.bounds())
        .unwrap_or_else(|| Bounds {
            origin: Point { x: px(0.0), y: px(0.0) },
            size: Size {
                width: px(1920.0),
                height: px(1080.0),
            },
        })
}

/// Build the [`WindowOptions`] for a new pin window: transparent, borderless,
/// screen-sized, floating, with `app-id = "pinapp"`.
///
/// Note: gpui-kit 0.7 exposes no runtime window-move/resize API, so each pin is
/// a full-screen transparent window and the image is positioned inside it (see
/// [`crate::pin_view`]).
fn build_window_options(cx: &App) -> WindowOptions {
    let screen = primary_screen_bounds(cx);
    let bounds = Bounds {
        origin: Point { x: px(0.0), y: px(0.0) },
        size: screen.size,
    };

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
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
        display_id: None,
        icon: None,
        tabbing_identifier: None,
    }
}
