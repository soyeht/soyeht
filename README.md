# Soyeht

Soyeht runs your coding agents as a team.

Each harness gets its own terminal pane. Agents discover and message each other over MCP. Sessions survive app restarts and reboots. Any conversation can be continued from your phone.

## Why

One agent is a chat. Five harnesses running side by side are an operations problem: who is working, who is blocked, who should review whose change, and how the work keeps going after you close the laptop. Soyeht is the layer that answers those questions.

## What it does

- **Orchestration over MCP.** Agents use MCP tools such as `list_agents` and `message_agent` to find each other, hand off work, and wait for replies. You set roles, communication policies and races between panes; every agent has an inbox.
- **Task list and loops.** Every workspace has a task list. Agents pull the next task, or you hand one to them; each task has one owner at a time, and a task goes back to the list if its agent dies. A loop delivers a prompt you wrote to a pane on the schedule you choose, so an agent keeps working through the list on its own.
- **Any harness, any model.** Soyeht does not replace your agent. Each pane runs the harness you already use (Claude Code, Codex, Gemini CLI, OpenCode, or anything that runs in a terminal) with the model it is configured for. Soyeht observes and coordinates.
- **Workspaces and panes.** Tabs of workspaces, panes side by side, one harness or shell per pane. Layout is persisted.
- **A real terminal, plus a view over it.** The raw terminal is always there. On top of it, Soyeht reads each harness's structured output to show threads, tasks and the harness's state: working, blocked, idle.
- **Continue from your phone.** Pick up any conversation on your iPhone and keep going.

## Status

Early. macOS is the supported platform. The engine builds on Linux, but Linux is not supported yet. Windows comes later. There is no release yet; installation instructions will land with the first one.

## Architecture

- `engine/` (Rust): terminal supervisor, workspace state, orchestrator and identity, behind one HTTP and WebSocket API. The same binary exposes the MCP server and a CLI.
- `desktop/` (TypeScript): the desktop app for Linux and Windows, a client of the engine API.
- The macOS and iOS apps are native Swift, live in their own repository, and speak the same API.
- Connectivity between your devices goes over Tailscale.

## Contributing

Every feature starts in `docs/spec.md`; if it is not in the spec, it is not in scope yet. Open an issue before a large change. CI must be green to merge.

## Developing

Everything in this repository is written in English: code, comments, docs, ADRs and commit messages.
Install the pinned toolchains from [docs/toolchains.md](docs/toolchains.md), then run from the repository root:

```sh
python3 contracts/generate.py --check
python3 -m unittest discover -s contracts -p 'test_*.py'
python3 -m unittest discover -s scripts -p 'test_*.py'
cargo fmt --manifest-path engine/Cargo.toml --all --check
cargo clippy --manifest-path engine/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo build --manifest-path engine/Cargo.toml --locked --workspace
cargo test --manifest-path engine/Cargo.toml --locked --workspace
bash scripts/check_swift.sh
swift format lint --strict --recursive apps/shared/Sources apps/shared/Tests apps/mac/Sources apps/ios/Sources
swift test --package-path apps/shared
bash scripts/check_mac.sh
bash scripts/check_ios.sh
python3 scripts/e2e.py --report .cache/e2e.json
npm --prefix contracts/typescript ci --ignore-scripts
npm --prefix contracts/typescript run check
npm --prefix contracts/typescript test
```

The engine tests use real processes in their own temporary directories. The six launchd acceptance tests run in CI. Physical reboot, camera pairing and cellular reads follow the manual acceptance script in `docs/`.

Changes land through pull requests with green checks; there is no direct push to `main`. The `forbidden-terms` check reads a repository secret and fails closed when the secret is unavailable.


## License

MIT. See `LICENSE`.
