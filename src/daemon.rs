//! Daemon: owns the GPUI application, serves IPC commands, and opens pin
//! windows on demand.
//!
//! Architecture:
//!   * a dedicated OS thread accepts Unix-socket connections, reads a single
//!     command line, and forwards `(command, reply_sender)` into an
//!     [`async_channel`] [`Receiver`];
//!   * the GPUI app spawns a background task that `recv`s those commands and,
//!     on the main thread, reads the clipboard and opens pin windows (GPUI
//!     windows may only be created from the main thread / an `update` call);
//!   * after processing a command, the main thread sends the outcome string
//!     ("ok" or "err:<message>") back through the reply channel; the listener
//!     thread writes it to the client as an acknowledgement, giving the user
//!     feedback when e.g. the clipboard holds no image.

use anyhow::Result;
use async_channel::unbounded;
use gpui_kit::{
    App, AppContext, Bounds, PlatformDisplay, Point, Pixels, Size, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::mpsc;

use crate::clipboard;
use crate::ipc;
use crate::pin_view::PinView;

/// Run the persistent daemon. Blocks until the application quits.
pub fn run_daemon() -> Result<()> {
    let (tx, rx) = unbounded::<(String, mpsc::Sender<String>)>();

    // OS thread that accepts IPC connections and forwards commands + reply
    // channels.
    start_socket_listener(tx)?;

    let _ = gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);

        // Background task: process IPC commands on the main thread so we can
        // open GPUI windows safely.
        let rx = rx;
        cx.spawn(|mut async_cx| async move {
            while let Ok((cmd, reply)) = rx.recv().await {
                match cmd.as_str() {
                    "pin" => match clipboard::read_clipboard_image() {
                        Ok(img) => {
                            let opened = async_cx.update(|cx| {
                                let opts = build_window_options(cx);
                                let _ = gpui_kit::open_window(opts, cx, |_, cx| {
                                    cx.new(|cx| PinView::new(img, cx))
                                });
                            });
                            let ack = match opened {
                                Ok(_) => "ok".to_string(),
                                Err(e) => format!("err:{e}"),
                            };
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
            if tx.send((line, reply_tx)).is_err() {
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
