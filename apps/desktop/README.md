# Soyeht desktop client

The desktop client for Linux and Windows, written in Rust with GPUI (the UI
framework of the Zed editor). It is another client of the engine's protocol: it
depends on `engine/soyeht-protocol` and shares no message type by copy. The
decisions are in `docs/adr/0084-desktop-client-gpui.md`.

It also builds and runs on macOS, which is how it is developed.

## Layout

| Crate | Role |
| --- | --- |
| `crates/soyeht-client` | Engine connection, handshake, terminal streams, client state (workspaces, panes, split layout, agent transcripts, usage), terminal screen, key encoding. No window system. |
| `crates/soyeht-desktop` | The GPUI window: design tokens (`theme`), neumorphic surfaces (`neo`), the top bar, rail, pane cards, agent panes and composer (`views`), terminal painter. |

This is its own cargo workspace with its own `Cargo.lock`; `engine/` does not
build it and it does not build the engine.

## What works

The window is laid out as the Mac app is: a top bar of workspace tabs (rounded pills with a
close mark, in a strip that scrolls sideways with a fade and a chevron over each end that hides
tabs, a "+N" pill listing the tabs that are not wholly in view, a "+" that opens a terminal or an
agent), the rail down the left (the places, the usage rings with their percentages, settings),
and the panes of the selected workspace as neumorphic cards with a flat interior. The agents'
place and the workspace tabs carry a badge with the requests agents wait on, or a dot while an
agent works.

- Connects to a running engine on its local endpoint, as the Mac app does (a Unix socket on
  Linux and macOS, the engine's loopback endpoint file on Windows, through `soyeht-platform`),
  and reconnects when the engine restarts.
- Panes are tiled as the workspace's split tree says (`workspace.layout`), and the split buttons
  of a pane header split it to the right or below and store the new arrangement.
- Terminal panes show live output (the engine streams raw terminal bytes, parsed
  with `vt100`), accept typing, paste, resize with the window, and scroll back.
- Agent panes show the transcript (the person's messages, replies drawn as Markdown, thoughts, tool rows, plans,
  command output in a mono well, hand-overs), take a
  message from the composer (text comes through GPUI's input handler, so dead keys, input
  methods and emoji work), and offer the harness, model, effort and permission pickers, the
  context ring and send or stop. Permission requests are answered in a card that shows the exact
  command, tool arguments and file changes. The header names the agent, the current plan step and
  the latest tool calls (the whole line is in its tooltip), and counts the running turn.
- A pane opened from "+" or a split takes the keyboard.
- Settings: the Appearance panel (a card with a swatch for each of the nine palettes) and the
  engine connection.
- A terminal pane says when its process has exited or has gone quiet, and when output was not
  recorded, in the lines the Mac shows above and below its screen.
- On Linux and Windows the top bar carries the window controls (minimize, maximize, close),
  drags the window, and on Linux the window edges resize it. The window opens no larger than
  its display. A tiling desktop (Hyprland, sway and the like, told by `XDG_CURRENT_DESKTOP`)
  ignores minimize and maximize, so a window there gets only the close button and no resize
  edges; a window snapped to half of the screen on GNOME or KDE keeps the buttons and loses the
  resize edges. The window announces itself as `soyeht` (Wayland `app_id`, X11 class) for window
  rules.
- A pane never gets narrower than its composer's controls need. When the panes need more room
  than the window has, the stage scrolls sideways (the wheel moves it only sideways, so
  transcripts and terminals keep their own scrolling) and fades at each end that hides more;
  the pane that was focused or just opened is scrolled into view. The composer keeps its chips at
  full size and scrolls them sideways in the same way.
- The "+" menu and the harness chip list the installed agents by readiness, each with how ready
  it is, and the others under a collapsed "More harnesses" entry; an agent that cannot start is
  shown dimmed. Popup menus are as tall as the Mac's popover at most, scroll inside, and close on
  Escape.
- Editor, web and writing panes, tasks, loops, attachments and dictation are listed with a notice.

## Run

Start the engine first (`docs/dev-local.md`), then:

```sh
cd apps/desktop
cargo run --release -p soyeht-desktop
```

The socket is found from the Dev profile directory under the user's home, which is where the
engine keeps it on macOS, Linux and Windows. To reach another engine, set the socket path:

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
palettes and tokens in `theme` are generated from `contracts/design-tokens.json`.

## Neumorphic surfaces

`neo.rs` draws the raised, inset and pane-shell surfaces from the same tokens as the Mac. GPUI's
`blur_radius` is a Gaussian's standard deviation, so it is half the token's blur (the design
tool's blur). The Mac stacks its two shadows, so its dark shadow is cast by the light halo too; the
desktop emulates that with three shadows whose scales are in the `desktop` section of the tokens
(`shadow*Scale`, `shadow*Opacity`), fitted against renders of the Mac's own `neoRaised`. The wells
(`neoWell`) use the plain inset pair. GPUI has no squircle corners, no blend modes and no rounded
clipping of children, so corners are circular arcs and content that touches a rounded corner is
rounded by its own radius.

`cargo run --release --example neo_gallery -- milk` opens every surface on one canvas at fixed
positions (no text), for comparing with the Mac's render of the same surfaces pixel by pixel.

## Rules

- Colors and sizes come only from `theme`, generated from `contracts/design-tokens.json`
  (also the source of the Swift palette). No literals.
- Only the pane shell is raised; everything inside a pane is flat.
- Wire types, framing and method names come from `soyeht-protocol`. A missing
  engine capability is a contract change, not a client-side guess.
- GPUI is pinned to one commit; an upgrade is its own pull request.

## Comparing with the design and the Mac

`examples/design_audit.rs` opens the window on a recorded engine state
(`examples/design_audit.json`: workspaces, panes, a transcript for each agent, permission requests,
terminal output, usage and harnesses, all in the wire's own shapes), with no engine:

```sh
cargo run --release --example design_audit -- crates/soyeht-desktop/examples/design_audit.json milk \
  --workspace ws-ci --size 1200x700 --open config:claude:model
```

`--open` takes `new-pane`, `overflow`, `settings`, `harness:<pane>` or `config:<pane>:<option>`.
The same file feeds the Mac's own views, rendered offscreen, so that both clients draw the same
conversation. The `.pen` frames "Interior da pane" (approved) and `DCzvR`, `OtYMZ` are the
references for the interior, the composer and the header.

## Assets

Fonts and the provider logos are the Mac app's own resources, embedded from `apps/mac` and
`apps/shared`, so no second copy exists to drift.

- The fonts (Inter, Nunito, JetBrains Mono) are under the SIL Open Font License 1.1. GPUI on Linux
  draws a variable font at its default weight, so the medium, semibold, bold and extra bold cuts of
  Inter are instanced from the Mac's variable `Inter.ttf` by `scripts/instance_inter_fonts.py` and
  kept in `crates/soyeht-desktop/assets/fonts`. The license
  texts that must travel with a binary are in `crates/soyeht-desktop/assets/licenses/`.
- The line icons in `crates/soyeht-desktop/assets/icons` are Lucide: ISC license, and the MIT
  license for the icons derived from Feather. Both are in
  `crates/soyeht-desktop/assets/LICENSE-lucide.txt`. They are the closest open equivalent of the
  Mac client's system symbols, which cannot be redistributed.
- The provider logos are the marks of their owners. They are drawn only to identify a provider's
  agent and usage, as in the Mac app, and are not covered by the licenses above.
- Desktop-only measurements (window frame, popups, glyph sizes, the line height of each face: GPUI's
  default line is 1.618 times the size, SwiftUI's is the font's own) are in the `desktop` section of
  `contracts/design-tokens.json`. They are generated for Rust only: the Mac output does not
  mention them and does not change when they do.
