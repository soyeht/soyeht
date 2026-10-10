# Soyeht desktop client

The desktop client for Linux and Windows, written in Rust with GPUI (the UI
framework of the Zed editor). It is another client of the engine's protocol: it
depends on `engine/soyeht-protocol` and shares no message type by copy. The
decisions are in `docs/adr/0084-desktop-client-gpui.md`.

It also builds and runs on macOS, which is how it is developed.

## Layout

| Crate | Role |
| --- | --- |
| `crates/soyeht-client` | Engine connection, handshake, terminal streams, client state, terminal screen, key encoding. No window system. |
| `crates/soyeht-desktop` | The GPUI window: palette tokens (`theme`), pane shell, terminal painter, views. |

This is its own cargo workspace with its own `Cargo.lock`; `engine/` does not
build it and it does not build the engine.

## What works

- Connects to a running engine on its local socket, as the Mac app does, and
  reconnects when the engine restarts.
- Lists workspaces and their panes; selecting a workspace shows its panes.
- Terminal panes show live output (the engine streams raw terminal bytes, parsed
  with `vt100`), accept typing, paste, resize with the window, and scroll back.
- New terminal in the selected workspace; close a pane.
- Agent, editor, web and writing panes are listed with a notice.

## Run

Start the engine first (`docs/dev-local.md`), then:

```sh
cd apps/desktop
cargo run --release -p soyeht-desktop
```

On macOS the socket is found from the Dev profile directory. Anywhere else, or
to reach another engine, set the socket path:

```sh
SOYEHT_ENGINE_SOCKET=/path/to/engine.sock cargo run -p soyeht-desktop
```

Shortcuts use Command on macOS and Control+Shift on Linux and Windows, so that
they never take keys a terminal program needs: New Terminal (`t`), Paste (`v`),
Quit (`q`).

GPUI needs the platform SDKs: Xcode with the Metal toolchain on macOS; Vulkan
drivers and the Wayland or X11 development libraries on Linux; the Windows SDK
on Windows.

## Check

```sh
cd apps/desktop
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

`scripts/check_all.sh` runs the same three steps. The first run on a machine
fetches the Zed git dependencies and compiles GPUI, which takes several minutes.
The client crate tests
include an end-to-end run against a scripted engine on a real local socket; the
theme test compares every color with `ThemePalette.swift`.

## Rules

- Colors come only from `theme`, which mirrors the Swift palette. No literals.
- Only the pane shell is raised; everything inside a pane is flat.
- Wire types, framing and method names come from `soyeht-protocol`. A missing
  engine capability is a contract change, not a client-side guess.
- GPUI is pinned to one commit; an upgrade is its own pull request.
