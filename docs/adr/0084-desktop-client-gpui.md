# ADR 0084: Desktop client for Linux and Windows, built with GPUI

- Status: accepted
- Date: 2026-10-08

## Context

The product has to run on Linux and Windows as well as macOS and iOS. The Mac and
iPhone apps are Swift clients of the engine; the engine itself is Rust and is
being ported to Linux and Windows. A desktop client for those two platforms
cannot reuse SwiftUI. It should be another client of the same protocol, share
the engine's wire types, and stay small enough that a feature is written once.

## Decision

### Why GPUI

The owner chose GPUI, the UI framework of the Zed editor. It is Rust, draws on
the GPU (Metal, Vulkan, Direct3D), runs on macOS, Linux (Wayland and X11) and
Windows, and was built for exactly the surfaces Soyeht needs next: a terminal
grid, long text views for the editor pane, and a tree of panes. A Rust client
links `soyeht-protocol` directly, so the wire types, framing and method names
come from the contract and are never copied. macOS support lets the client be
developed and run on the host Mac before the Linux and Windows machines are
involved.

The cost is that GPUI is not published as a stable crate. It is a git
dependency on the Zed repository, its API changes between releases, and its
platform back ends have their own build requirements.

### How GPUI is pinned

`gpui` and `gpui_platform` are git dependencies on `zed-industries/zed`, pinned
to an exact commit: `ced90fc636c4ede05402befc38a63bae7fd741bd`, the commit of the
Zed `v1.9.0` release tag. A tag is not used, because a tag can be moved; the
commit cannot. The two revisions are written once, in the workspace
`Cargo.toml`, and the lockfile is committed.

GPUI depends on forks that Zed applies with `[patch.crates-io]` in its own
workspace, and Cargo ignores the patches of a dependency's workspace. The two
that GPUI needs (`async-task`, `calloop`) are repeated in the desktop
workspace, each at the exact revision recorded in Zed's lockfile for that
commit. An upgrade moves the GPUI revision and these two together, and is its
own pull request.

### Separate cargo workspace

`apps/desktop` is its own cargo workspace with its own `Cargo.lock`; it is not a
member of `engine/`. Reasons:

- The engine's own steps in the gate (`cargo clippy --locked --workspace`,
  build, test, run inside `engine/`) do not compile GPUI, which is a large
  dependency tree with platform SDK needs, and the engine's lockfile does not
  absorb it.
- GPUI needs patched forks that do not belong in the engine's dependency graph.
- The layering of `AGENTS.md` already puts clients above `soyeht-protocol`; a
  client depends on the protocol crate by path (`../../engine/soyeht-protocol`)
  and on the platform crate (`../../engine/soyeht-platform`, ADR 0086), which owns the
  local endpoint: a Unix socket on Linux and macOS, and on Windows the loopback endpoint
  file the engine writes. The client reaches the engine through it so that no system has
  its own copy of that transport. It depends on nothing else in `engine/`.

The desktop workspace keeps the engine's lints (`unwrap_used`, `expect_used` and
`panic` denied; tests exempt), edition and toolchain.

The desktop client is gated like every other client. `scripts/check_all.sh` runs
its format, clippy and test steps in `apps/desktop` after the engine's, and so a
gate run that selects the desktop area does compile GPUI. The first run on a machine fetches the Zed git
dependencies (the GPUI revision and the two patched forks) and builds them, which
takes several minutes and needs network access and the platform SDKs; later runs
reuse the cache. The quality ratchets (`scripts/check_quality.py`: clones, file
size, literals, swallowed errors, cargo-machete, and the production-panic ceiling)
cover `apps/desktop` with the engine, and the fast pre-push check needs no GPUI
build.

### Architecture

Two crates, so that everything except painting builds and tests without a
window system:

1. `soyeht-client`: the engine connection and the client state.
   - `transport`: the local socket, two halves and a closer. Framing is the
     protocol crate's `Frame`.
   - `rpc`, `session`: request and response correlation, the `hello` and
     `state.snapshot` handshake, and the reader thread that routes every frame
     (responses, notifications, terminal output). Requests are the protocol
     crate's typed `Request` enum, so method names are the generated ones. The
     greeting and `hello` checks (`soyeht_protocol::handshake`) and the assembly
     of snapshot pages (`soyeht_protocol::snapshot`) are the protocol crate's,
     shared with `soyeht-cli` and `soyeht-mcp`; the client only reads the
     sections it renders.
   - `streams`: terminal stream positions, the same rules as the Swift
     `TerminalStreamReplica` (reference and stream indexes, offsets,
     generations, gaps).
   - `engine`: a supervisor thread that reconnects every second, and one worker
     thread that runs commands in order, merging keystrokes and resizes.
   - `state`: the replica a view renders. It applies `Event` values and returns
     `Effect` values (for example "subscribe this terminal"), so each rule is a
     plain unit test.
   - `screen`, `keys`: the engine streams raw terminal bytes, as it does to the
     Mac client; `screen` feeds them to the `vt100` parser already in the engine
     workspace and exposes styled runs, and `keys` encodes key presses.
2. `soyeht-desktop`: the GPUI window.
   - `theme`: the palette tokens, one module (see Design).
   - `pane_shell`, `terminal_view`, `views`, `workbench`: the raised pane
     container, the painter of a terminal grid, rendering, and the root view
     that owns the connection and turns input into commands.

The connection is plain threads and an `async-channel` into GPUI's executor. A
views layer never touches a socket and the client crate never imports GPUI.

Both queues are bounded. When the interface cannot draw as fast as a program
prints, the event queue fills and the reader thread waits, which stops it taking
bytes off the socket; memory stays bounded and the engine's own queue limits take
over. The command queue refuses a command when it is full instead of blocking the
interface, and the interface says so. An event the client cannot read has already
moved the event cursor, so the state drops the connection and starts again from a
fresh snapshot, as the Swift client does when an event fails to decode.

The client connects the way the Mac client does: the engine's local socket,
`auth.challenge`, `hello` as `human_local`, `state.snapshot`, `events.subscribe`
from the snapshot cursor, then `pane.stream.subscribe` from the tail for each
terminal. Typing is `pane.input` in base64; the view size is `pane.resize`.

### Design

Colors come only from the palette (ADR 0065). The palettes are not hand-copied: both clients are generated from
`contracts/design-tokens.json`, and `theme` holds all nine themes as generated
data, with Neo Milk the seeded default. Views take roles from the active palette and
never write a literal. The pane shell is the only raised surface: `face`, radius
20, two outer shadows at (5, 5) and (-5, -5) with blur 10, as in
`PaneShellMetrics`. Everything inside a pane is flat. Fonts are the approved
Inter and JetBrains Mono, embedded from `apps/mac/Resources/Fonts` so there is no
second copy.

The sixteen ANSI colors of a terminal come from the palette. Indexed colors
16 to 255 follow the xterm cube and gray ramp, because programs address them by
number; true color passes through. These are terminal content, not interface
colors.

### Platforms

- macOS: built and run on the host for development. It is not a shipped target
  of this client; the Swift app is.
- Linux (Wayland and X11) and Windows 11: the shipped targets. Both compile GPUI
  from the same revision, and the client has been run on Ubuntu, Fedora, Omarchy
  (Hyprland) and Windows 11 against a disposable engine. `scripts/check_engine_linux.sh`
  and `scripts/check_engine_windows.ps1` format-check, lint and test it there.

The engine's socket is found in the data directory of the client's profile, below the
system's native data base (`soyeht_platform::dirs`) on every system (the generated
`soyeht_protocol::profile` constants, ADR 0112); `SOYEHT_ENGINE_SOCKET` overrides it. The local transport is `soyeht_platform::local` (ADR 0086): a Unix socket,
or on Windows the loopback endpoint file with its proof of the shared secret.

Window chrome follows the window system. The application draws its own title bar, so on
Linux and Windows the top bar carries the window buttons. A tiling desktop (Hyprland, sway
and the like, told by `XDG_CURRENT_DESKTOP`) ignores minimize and maximize, so it gets only
the close button; a tiled edge only removes the resize edges, because a window snapped to
half of the screen on GNOME or KDE can still be minimized and maximized.

## Consequences

- The desktop client is a client of the protocol like any other. Anything it
  needs from the engine is a contract change first.
- GPUI upgrades are deliberate: one revision, two patch revisions, one pull
  request.
- Scope of the first version: workspaces, panes, live terminal panes with
  typing, paste, scrollback and resize. Agent panes (transcript, composer and its
  pickers, harness switch) and the editor pane (see `apps/desktop/README.md`) came with the Mac parity work; web and
  writing panes show a notice. Text selection and copy, IME composition, the Mac's harness
  switch sheet (explanation, model and effort before switching) and account section,
  and workspace creation are later.
- `input chunk size` (16 KiB per `pane.input`) is a Mac client convention, not a
  contract limit; it should move to `contracts/protocol.json`.
