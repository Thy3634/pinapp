//! pinapp — pin clipboard images as always-on-top floating windows.
//!
//! Usage:
//!   * `pinapp`            → run the daemon (also `pinapp --serve`)
//!   * `pinapp --pin`       → tell the running daemon to pin the current clipboard image
//!   * `pinapp --quit`      → stop the daemon
//!
//! The daemon owns the GPUI application. Pressing the Pause key (bound in niri)
//! runs `pinapp --pin`, which sends a command over a Unix socket; the daemon
//! reads the clipboard and opens a new floating window for the image.

mod clipboard;
mod daemon;
mod image_util;
mod ipc;
mod pinned_image;
mod pin_view;

use anyhow::Result;

fn main() -> Result<()> {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        // No argument, or explicit --serve: run the persistent daemon.
        Some("--serve") | None => {
            daemon::run_daemon()?;
        }
        // Ask the daemon to pin the current clipboard image.
        Some("--pin") => {
            ipc::send_command("pin")?;
        }
        // Ask the daemon to quit.
        Some("--quit") => {
            ipc::send_command("quit")?;
        }
        Some(other) => {
            eprintln!("pinapp: unknown argument `{other}`");
            eprintln!("usage: pinapp [--pin | --quit | --serve]");
        }
    }
    Ok(())
}
