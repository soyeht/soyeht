# Soyeht

Soyeht runs your coding agents as a team.

Each harness gets its own terminal pane. Agents discover and message each other over MCP. Sessions survive app restarts and reboots. Any conversation can be continued from your phone.

## Why

One agent is a chat. Five harnesses running side by side are an operations problem: who is working, who is blocked, who should review whose change, and how the work keeps going after you close the laptop. Soyeht is the layer that answers those questions.

## What it does

- **Orchestration over MCP.** Agents use MCP tools such as `list_agents` and `message_agent` to find each other, hand off work, and wait for replies. You set roles, communication policies and races between panes; every agent has an inbox.
- **Task list and loops.** Every workspace has a task list. Agents pull the next task, or you hand one to them; each task has one owner at a time, and a task goes back to the list if its agent dies. A loop delivers a prompt you wrote to a pane on the schedule you choose, so an agent keeps working through the list on its own.
- **Any harness, any model.** Soyeht does not replace your agent. Each pane runs the harness you already use (Claude Code, Codex, Gemini CLI, OpenCode, or anything that runs in a terminal) with the model it is configured for. Soyeht observes and coordinates.
- **Skills in every harness.** Keep reusable `SKILL.md` skills in one place. Claude Code and Codex load them natively, typing "/" in the composer runs one, and any agent can write a new skill when you ask it to.
- **Workspaces and panes.** Tabs of workspaces, panes side by side, one harness or shell per pane. Layout is persisted.
- **A real terminal, plus a view over it.** The raw terminal is always there. On top of it, Soyeht reads each harness's structured output to show threads, tasks and the harness's state: working, blocked, idle.
- **Continue from your phone.** Pick up any conversation on your iPhone and keep going.

## Status

Early. macOS is the supported platform. The engine and the Linux and Windows app build and pass their checks on those systems, but they are not supported yet. The iPhone, iPad and Android apps are companions of an engine running on your computer. There is no release yet; installation instructions will land with the first one.

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
