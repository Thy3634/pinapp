//! IPC client: send `pin` / `quit` commands to the daemon over a Unix socket.
//!
//! If the daemon is not already running, this module launches `pinapp --serve`
//! detached and retries the connection for a short grace period, so a bare
//! `pinapp --pin` from a key binding "just works".

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// Socket file name inside `$XDG_RUNTIME_DIR`.
const SOCKET_NAME: &str = "pinapp.sock";

/// Path of the daemon's Unix socket (`$XDG_RUNTIME_DIR/pinapp.sock`).
pub fn socket_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime).join(SOCKET_NAME)
}

/// Send a single-line command to the running daemon, launching it if needed.
pub fn send_command(cmd: &str) -> Result<()> {
    let path = socket_path();

    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => {
            // Daemon not running yet — start it and retry for a short while.
            launch_daemon().context("launch pinapp daemon")?;
            let mut connected: Option<UnixStream> = None;
            for _ in 0..15 {
                std::thread::sleep(std::time::Duration::from_millis(200));
                match UnixStream::connect(&path) {
                    Ok(s) => {
                        connected = Some(s);
                        break;
                    }
                    Err(_) => continue,
                }
            }
            connected.context("connect to pinapp daemon after launch")?
        }
    };

    let line = format!("{cmd}\n");
    stream
        .write_all(line.as_bytes())
        .context("write command to socket")?;

    // The daemon replies with a single acknowledgement line ("ok" or
    // "err:<message>") and then closes the connection. Surface failures
    // (e.g. clipboard holds no image) instead of failing silently.
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(2000)))
        .ok();
    let mut ack = String::new();
    let _ = stream.read_to_string(&mut ack);
    let ack = ack.trim();
    match ack {
        "" => Ok(()), // daemon didn't reply (race) — assume ok
        s if s.starts_with("ok") => Ok(()),
        s => Err(anyhow::anyhow!("pinapp daemon: {s}")),
    }
}

/// Launch `pinapp --serve` detached (no inherited stdio).
fn launch_daemon() -> Result<()> {
    let exe = std::env::current_exe().context("resolve current executable")?;
    std::process::Command::new(&exe)
        .arg("--serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("spawn pinapp --serve")?;
    Ok(())
}
