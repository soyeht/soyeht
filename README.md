# Soyeht

Soyeht runs your coding agents as a team.

Each harness gets its own terminal pane. Agents discover and message each other over MCP. Sessions survive app restarts and reboots. Any conversation can be continued from your phone.

## Why

One agent is a chat. Five harnesses running side by side are an operations problem: who is working, who is blocked, who should review whose change, and how the work keeps going after you close the laptop. Soyeht is the layer that answers those questions.

## What it does

- **Orchestration over MCP.** Agents use MCP tools such as `list_agents` and `message_agent` to find each other, hand off work, and wait for replies. You set roles, communication policies and races between panes; every agent has an inbox.
- **Task list and loops.** Every workspace has a task list. Agents pull the next task, or you hand one to them; each task has one owner at a time, and a task goes back to the list if its agent dies. A loop delivers a prompt you wrote to a pane on the schedule you choose, so an agent keeps working through the list on its own.
- **Any harness, any model.** Soyeht does not replace your agent. Each pane runs the harness you already use (Claude Code, Codex, Cursor, OpenCode, or anything that runs in a terminal) with the model it is configured for. Soyeht observes and coordinates.
- **Skills in every harness.** Keep reusable `SKILL.md` skills in one place. Claude Code and Codex load them natively, typing "/" in the composer runs one, and any agent can write a new skill when you ask it to.
- **Workspaces and panes.** Tabs of workspaces, panes side by side, one harness or shell per pane. Layout is persisted.
- **A real terminal, plus a view over it.** The raw terminal is always there. On top of it, Soyeht reads each harness's structured output to show threads, tasks and the harness's state: working, blocked, idle.
- **Continue from your phone.** Pick up any conversation on your iPhone and keep going.

## Status

Soyeht 0.1.0 is the first public release. Releases will be published on the [releases page](https://github.com/soyeht/soyeht/releases); until the first one is there, build from source (below). It is early software: expect rough edges, and tell us about them (see "Privacy and support" below). The version history is in [CHANGELOG.md](CHANGELOG.md).

| System | What works today | What does not |
| --- | --- | --- |
| macOS 15 or later, Apple silicon | The full app: workspaces, terminals, agent panes, editor, texts, screenplays, presentations, web panes. Pairing an iPhone or iPad. | Intel Macs. |
| Windows 11 (x64 and Arm64) | The app has the same panes, including web panes on WebView2. The engine and the supervisor start with the window and can start at login. | Pairing a phone: the app cannot yet prove itself to the engine, so no pairing can be opened or approved ([ADR 0162](docs/adr/0162-pairing-is-decided-only-by-the-signed-app-on-every-system.md), [ADR 0163](docs/adr/0163-opening-a-pairing-window-is-the-signed-apps-alone.md)). A few features that need a macOS service report that they are unsupported ([docs/platforms.md](docs/platforms.md)). |
| Linux (x86_64 and arm64) | The same app as on Windows, with web panes on WebKitGTK. Checked on Ubuntu 26.04 (arm64). | The same as Windows. |
| iPhone and iPad (iOS 18 or later) | A companion of a Mac: the same terminals, agents and documents, with Apple Pencil handwriting. | Pairing with a Windows or Linux computer (above). |
| Android | Not released yet. | Everything. The code is in this repository ([docs/apps-android.md](docs/apps-android.md)). |

Not in 0.1.0 on any system: push notifications. The iPhone and iPad app does not ask for notification permission, and nothing is sent to Apple's push service. The Mac, Windows and Linux apps still raise a system notification on your computer when an agent needs you ([docs/push-notifications.md](docs/push-notifications.md)).

Four harnesses are supported today: Claude Code, Codex, Cursor and OpenCode. You install and sign in to each one yourself, under your own account and terms with its provider; Soyeht does not proxy those conversations ([PRIVACY.md](PRIVACY.md)). Any other program that runs in a terminal also works in a terminal pane.

### Install

Once a release is published, download the package for your system from the [releases page](https://github.com/soyeht/soyeht/releases) (the newest release is at `https://github.com/soyeht/soyeht/releases/latest`). The files are named `soyeht-<version>-<system>-<architecture>.<extension>`, and each release lists a `SHA256SUMS` file.

- **macOS:** open `soyeht-<version>-macos-aarch64.dmg` and drag the app to Applications. The app is signed and notarized.
- **Windows:** run `soyeht-<version>-windows-x86_64.exe` (or `-windows-aarch64.exe` on an Arm PC). It installs for your user and needs no administrator.
- **Linux:** `sudo apt install ./soyeht-<version>-linux-<arch>.deb`, or make `soyeht-<version>-linux-<arch>.AppImage` executable and run it. [docs/platforms.md](docs/platforms.md) lists the libraries the machine needs.
- **iPhone and iPad:** the App Store, at launch.

[docs/getting-started.md](docs/getting-started.md) goes from the first launch to a terminal, an agent and a paired phone. [PRIVACY.md](PRIVACY.md) says what leaves your devices, [SECURITY.md](SECURITY.md) how to report a vulnerability and [CONTRIBUTING.md](CONTRIBUTING.md) how to build and change Soyeht.

### Build from source

| Platform | App | Where | Build and test |
| --- | --- | --- | --- |
| macOS | Native Swift app | `apps/mac` | `bash scripts/check_mac.sh` ([docs/apps-mac.md](docs/apps-mac.md)) |
| Linux and Windows | Rust and GPUI app (also runs on macOS) | `apps/desktop` | `cd apps/desktop && cargo test --locked --workspace` ([apps/desktop/README.md](apps/desktop/README.md), [docs/platforms.md](docs/platforms.md)) |
| iPhone and iPad | Native Swift app | `apps/ios` | `bash scripts/check_ios.sh` ([docs/apps-iphone.md](docs/apps-iphone.md)) |
| Android | Jetpack Compose app over a Rust core | `apps/android` | `bash scripts/check_android.sh` ([docs/apps-android.md](docs/apps-android.md)) |

The engine itself builds on all three desktop systems: `cargo build --manifest-path engine/Cargo.toml --locked --workspace`.

## Architecture

Each layer depends only on the ones before it; `AGENTS.md` has the full map and the rules.

- `contracts/`: the protocol schema, the installed profiles, the product version and the design tokens. The single source for the types generated in Rust, Swift, TypeScript and Python (and, for the design tokens, Kotlin).
- `engine/` (Rust): the process supervisor, workspace state, orchestrator and identity, behind one local socket and WebSocket API. The same workspace builds the MCP server and the CLI.
- `apps/shared/` (Swift, `SoyehtClient`): the engine client, replicas, presentation policies and layout math used by the Mac and iPhone apps. `apps/dictation/` (Swift, `SoyehtDictation`) holds the dictation engines that need third-party dependencies.
- `apps/mac/` and `apps/ios/` (Swift): the native macOS and iPhone/iPad apps, thin views over `apps/shared/`.
- `apps/desktop/` (Rust, GPUI): the app for Linux and Windows, a client of the same API. It is its own cargo workspace.
- `apps/android/` (Kotlin and Rust): the Android app. `apps/android/app` is a Jetpack Compose view over `apps/android/core`, a Rust core that reuses the desktop client's connection and state.
- `docs/`: contracts (`docs/contracts/`), decisions (`docs/adr/`) and platform and dev notes. `scripts/` holds the merge gate and the build and test helpers.
- Connectivity between your devices goes over Tailscale.

### Pinned forks

Three dependencies are pinned to a git revision instead of a published version:

- the `vt100` crate in all three cargo workspaces (`engine`, `apps/desktop`, `apps/android`), through `[patch.crates-io]`, at a revision of [`soyeht/vt100-rust`](https://github.com/soyeht/vt100-rust): a compact scrollback and no panic on any terminal output ([ADR 0144](docs/adr/0144-the-windows-and-linux-window-and-the-android-core-use-a-vt100-fork-with-a-compact-scrollback.md));
- SwiftTerm, for the Mac and iOS terminals, at a tag of [`soyeht/SwiftTerm`](https://github.com/soyeht/SwiftTerm): the same compact scrollback ([ADR 0145](docs/adr/0145-the-mac-and-ios-terminals-keep-the-policy-scrollback-in-a-swiftterm-fork.md));
- GPUI, for `apps/desktop`, at the commit of a Zed release tag in `zed-industries/zed` ([ADR 0084](docs/adr/0084-desktop-client-gpui.md)), so every build gets the same GPUI.

Each fork is upstream plus a few commits, with its licence and history. A pin changes in a pull request that updates every place that names it; the vt100 revision must be the same in the three workspaces, and `scripts/check_layers.py` fails when it is not (it also compares their `rust-version` and lint tables).

## Contributing

Every feature starts in `docs/contracts/`; if it is not in a contract, it is not in scope yet. Open an issue before a large change. `CONTRIBUTING.md` explains how to build Soyeht on macOS, Linux and Windows, run it, test it and prepare a pull request; `docs/add-a-pane-type.md` and `docs/add-a-harness.md` are the guides for the two most common extensions. Report security problems as `SECURITY.md` says.

## Privacy and support

Soyeht collects no data and has no account or server of its own. `PRIVACY.md` lists everything that leaves your devices and why. For help, open an issue at <https://github.com/soyeht/soyeht/issues>; report security problems as `SECURITY.md` says.

## License

Apache-2.0. See `LICENSE` and `NOTICE`. The third-party provider logos are from Lobe Icons
(MIT); their trademarks belong to their owners.
