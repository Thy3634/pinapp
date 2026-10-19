# pinapp

> Pin the current clipboard image as an always-on-top, floating, transparent
> window — a minimal, open-source [Snipaste](https://www.snipaste.com/)-like
> tool for Linux **Wayland** (built and tested mentally against
> **danklinux / niri**).

You take a screenshot (on danklinux, `dms screenshot` copies the image to the
clipboard by default), then press a key — a floating, transparent window appears
with that image pinned on top of everything. Move it, scale it, rotate it,
adjust its opacity, flip it, or make it click-through. Close it with `Esc`.

---

## How it works

`pinapp` has two roles:

1. **Daemon** (`pinapp` or `pinapp --serve`) — owns the GPUI application, listens
   on a Unix socket (`$XDG_RUNTIME_DIR/pinapp.sock`), and opens a new floating
   window whenever it receives a `pin` command.
2. **Client** (`pinapp --pin` / `pinapp --quit`) — sends a one-line command to
   the daemon over the socket. If the daemon isn't running, the client launches
   `pinapp --serve` detached and retries, so a bare `pinapp --pin` from a key
   binding "just works".

On `pin`, the daemon reads an image from the clipboard (via `arboard`, with a
`wl-paste --type image/png` fallback) and opens a pin window for it.

### Window / click-through design (important)

gpui-kit **0.7** does not expose a runtime window-move/resize API
(`set_window_bounds` does not exist). To still support dragging, scaling,
rotating, flipping and click-through **without moving the OS window**, each pin
is implemented as a **transparent, borderless, screen-sized floating window**
(`app-id = "pinapp"`, `WindowKind::Floating`,
`WindowBackgroundAppearance::Transparent`). The image is drawn as an
absolutely-positioned element centered in that window.

Wayland click-through is done with `window.set_input_region`:

| Mode            | Input region                                  | Effect                                                        |
| --------------- | --------------------------------------------- | ------------------------------------------------------------- |
| Normal          | the image rectangle only                       | only the image catches the pointer; outside passes through     |
| Click-through   | empty (`Some(&[])`)                            | the whole surface passes through, including the image         |

So dragging/scaling/etc. operate on the in-window image element; the surrounding
transparent area never blocks the windows beneath it.

---

## Requirements

- A Wayland compositor (tested conceptually with **niri**).
- **Rust** toolchain (edition 2021) to build.
- Clipboard access:
  - `arboard` (bundled dependency) for X11 / some Wayland setups, **and/or**
  - [`wl-clipboard`](https://github.com/bugaevc/wl-clipboard) providing
    `wl-paste` — required on a pure Wayland session where arboard cannot reach
    the clipboard. `dms screenshot` (danklinux) populates this clipboard.
- GPUI backend libs for your platform (XKB, Wayland client libs, etc.). If the
  binary fails to start under Wayland, rebuild with the platform crate's
  `wayland` feature enabled (check the exact feature name with
  `cargo tree -p gpui-kit`).

> **Dependency note:** `gpui-kit` 0.7 bundles the GPUI engine (the `gpui-pre`
> 0.3.7 fork). There is **no** `gpui = "0.7"` crate on crates.io and gpui-kit
> does **not** expose a `wayland` Cargo feature, so the project depends only on
> `gpui-kit` (see `Cargo.toml`).

---

## Build

```sh
cargo build --release
# the binary is at target/release/pinapp
```

(Compilation must be done on your machine — the sandbox used to author this
project cannot compile Rust. Every gpui-kit API used here has been verified
against the exact versions a fresh `cargo build` resolves: `gpui-kit` **0.7.3**
→ engine `gpui-pre` **0.3.9** (Wayland + X11 backends on by default). Still
run `cargo build --release` on your machine — it is the real compile gate.)

---

## niri configuration

Add a **window rule** so pin windows float and a **key binding** for the Pause
key. The exact KDL syntax varies by niri version — verify against your installed
`niri` (e.g. `man niri` / `niri --help`) — but the intent is:

```kdl
// ~/.config/niri/config.kdl

// Make pinapp float (it already opens as a transparent, screen-sized window).
window-rule {
    match app-id="pinapp"
    open-floating true
}

// Pause -> take a screenshot into the clipboard, then pin it.
// (dms screenshot copies the image to the clipboard by default on danklinux.)
// Every spawn argument goes in its own quotes.
binds {
    Pause { spawn "sh" "-c" "dms screenshot && pinapp --pin"; }
}
```

On niri >= 25.08 you can use the shell helper instead:

```kdl
binds {
    Pause { spawn-sh "dms screenshot && pinapp --pin"; }
}
```

If you prefer to keep **Print** for the screenshot and **Pause** only for
pinning (the daemon reads whatever image is already on the clipboard):

```kdl
binds {
    Pause { spawn "pinapp" "--pin"; }
}
```

…provided your screenshot tool already placed an image on the clipboard before
the pin runs (bind the screenshot key separately, or chain it as shown above).

> niri has no built-in "always on top across all workspaces" toggle; a floating
> window floats above **tiled** windows in the current workspace. That is the
> expected behavior here.

---

## Autostart (optional)

Let the daemon start with your session so the first `pinapp --pin` is instant.
Pick one:

- **niri autostart** (if your niri supports it):
  ```kdl
  autostart {
      "pinapp" "pinapp" "--serve"
  }
  ```
- **systemd --user** drop-in `~/.config/systemd/user/pinapp.service`:
  ```ini
  [Unit]
  Description=pinapp clipboard-pin daemon

  [Service]
  ExecStart=%h/.cargo/bin/pinapp --serve
  Restart=on-failure

  [Install]
  WantedBy=default.target
  ```
  then `systemctl --user enable --now pinapp`.

The client's auto-launch fallback means autostart is convenience, not a hard
requirement.

---

## Usage & interaction reference

1. Take a screenshot (e.g. `dms screenshot`) so an image is on the clipboard.
2. Press the bound key (Pause) → `pinapp --pin` → a floating image appears.
3. Interact with it:

| Action                       | Input                                   |
| ---------------------------- | --------------------------------------- |
| Move                         | Left-click drag                         |
| Scale up / down              | Mouse wheel up / down                   |
| Opacity up / down            | **Ctrl** + wheel up / down              |
| Rotate clockwise             | `R`                                     |
| Rotate counter-clockwise     | `Shift` + `R`                           |
| Flip horizontally            | `F`                                     |
| Flip vertically              | `V`                                     |
| Toggle click-through         | `T`                                     |
| Close                        | `Esc`                                   |

Open as many pins as you like; each is its own window. `pinapp --quit` stops the
daemon (closes everything).

---

## Known limitations & risks

- **Build/API drift:** written against gpui-kit 0.7.0 / gpui-pre 0.3.7 API docs.
  It has **not** been compiled in the authoring sandbox — run
  `cargo build --release` and reconcile any signature differences for your exact
  patch version (notably `cx.new` vs `cx.new_view` and `Application::run`'s
  return type, both handled defensively in the code).
- **`WindowDecorations` has no `Borderless` variant** in gpui-kit 0.7; borderless
  is achieved with `window_decorations: None` (no server/client decorations).
- **No runtime window move/resize:** hence the full-screen transparent-window +
  input-region approach described above (intentional, documented design).
- **True "always on top":** depends on the compositor; niri floats it above
  tiled windows in the current workspace only.
- **Daemon lifetime:** gpui typically quits the app when the last window closes.
  If you close every pin, the daemon may exit; the next `pinapp --pin`
  auto-relaunches it. A pending IPC task also helps keep it alive while pins
  exist. If you need a strict always-alive daemon, add an off-screen keep-alive
  window or a systemd service (see Autostart).
- **Keyboard focus:** key bindings assume the pin window/view is focused (the
  default for a freshly opened window). If keys don't respond, click the image
  once to focus it.
- **`set_input_region` during `render`:** applied idempotently each paint (like
  `set_cursor_style`); if a future gpui-kit disallows window mutation in `render`,
  move the call into an `on_next_frame`/window-event hook.
- **Clipboard on pure Wayland:** relies on `wl-paste` (wl-clipboard) when arboard
  can't reach the clipboard.

---

## File layout

```
pinapp/
├── Cargo.toml            # package + dependencies (gpui-kit 0.7, arboard, image, ...)
├── README.md             # this file
└── src/
    ├── main.rs           # argument parsing / dispatch
    ├── daemon.rs         # GPUI app, Unix-socket server, window options
    ├── ipc.rs            # client: send pin/quit over the socket (+ auto-launch)
    ├── clipboard.rs      # clipboard image read (arboard + wl-paste fallback)
    ├── pin_view.rs       # PinView: render + interactions + click-through
    └── image_util.rs     # cache dir, PNG (re)encode, rotate/flip/opacity
```
