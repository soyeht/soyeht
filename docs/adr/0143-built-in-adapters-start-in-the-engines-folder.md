# ADR 0143: Built-in adapters start in the engine's folder

- Status: accepted. Extends ADR 0079 (external commands off the Core lock), ADR 0102 (the launch
  path) and ADR 0137 (a pane runs the Claude Code the person has).
- Date: 2026-11-05

## Context

A built-in agent of the registry is launched through a package runner: `npx --yes <package>@<version>`
or `uvx <package>==<version>`. The version is pinned, but the runner also reads the folder it starts
in: an `.npmrc` that names another registry, a `node_modules` that holds the package, a
`.python-version` or `uv.toml`. The engine started the adapter in the pane's folder, which is the
workspace, so a repository could decide which code the pinned adapter was.

## Decision

**A built-in adapter launched through a package runner starts in `<data directory>/adapters`.** The
folder is the engine's: owner-only, made where the start is sent, off the Core lock, before each
start (so one that was removed is made again), empty. This holds for the agent process, the candidate process of a harness switch and the
sign-in terminal an adapter asks for.

- **One rule.** `agents/package_runner.rs` says what a package runner is (`package_runner`, by file stem:
  `npx`, `npx-cli`, `uvx`, `uv`), which launches are built-in (`registry.` ids) and which of those
  start in the engine's folder (`starts_in_engine_folder`). The sign-in terminal reads the runner
  from the catalogue's entry, the same value that builds its arguments.
- **Custom harnesses are left alone.** A custom entry that runs `npx ./local-agent` or `uv run
  script.py` keeps the pane's folder, and its relative paths keep meaning what its author wrote.
- **The session's folder is unchanged.** The adapter receives the workspace as `cwd` in ACP
  `session/new` and `session/load` and works there.

## What was checked

Whether an adapter takes its workspace from the session's `cwd` and not from the folder its process
started in, read from the public source of the pinned version, with nothing installed:

- **claude-agent-acp 0.100.0**: the session uses the request's `cwd`. `process.cwd()` is used only
  for the authentication probe before a session exists and for `apiKeyHelper`, so an
  `apiKeyHelper` set by the project is not seen before the first session. That is the one
  behaviour that changes, and it is the intended one: a project must not choose a program the
  adapter runs before the person has opened its session.
- **codex-acp 1.11.0**: every thread call passes the request's `cwd`.
- **gemini-cli 0.59.0**: loads its settings by the session's `cwd`.
- **pi-acp 0.0.33**: `session/new` and `session/load` require an absolute `cwd`, load the project's
  prompts, skills and settings from it and start the agent process in it; no `process.cwd()`.
- **cline 3.0.61**: the session keeps the request's `cwd`; `process.cwd()` is only a fallback for
  an empty one. How the files it loads are resolved was not read to the end.
- **fast-agent-acp 0.10.1**: the session's `cwd` comes from the request, but the project
  configuration (`fast-agent.yaml`) is found from the folder the process starts in
  (`find_config_in_directory(start_path)`, `Path.cwd()`), so in the engine's folder the person's
  project configuration would be lost. It is **excluded**: `READS_PROJECT_FROM_ITS_FOLDER` lists it
  and it starts in the workspace as before. Its `uvx` run still reads the workspace's `uv`
  configuration; this is a known gap, closed when the adapter reads its project from the session.
- **Not checked**: the other `npx` and `uvx` entries of the snapshot (agoragentic-acp, auggie,
  autohand, codebuddy-code, deepagents, dimcode, dirac, factory-droid, github-copilot-cli,
  glm-acp-agent, grok-build, minimax-code, minion-code, nova, qoder, qwen-code). They start in the
  engine's folder; one found to read its project from the process folder is added to the exclusion.

## Consequences

- A repository's package-runner configuration no longer reaches a built-in adapter's start.
- An adapter that reads project files from its process folder must be listed in
  `READS_PROJECT_FROM_ITS_FOLDER`.

## Note for 0.1.0: only the checked harnesses are listed

The "Not checked" entries above, and every other built-in agent of the snapshot, have not been run
against the real program the way `claude`, `codex`, `cursor` and `opencode` have
(`scripts/e2e_real_harnesses.py`, the `harnesses` of `x-readiness`). Until one is checked, the
engine does not offer it to a person.

- **One marker, in the catalogue data.** Each entry of `acp_registry_snapshot.json` says
  `"support": "supported"` or `"support": "experimental"`; the field is required, so a new entry has
  to choose. The four checked harnesses are the supported ones, and a test holds them equal to the
  harness ids of `x-readiness`. No client keeps a list.
- **Hidden, not removed.** `harness.list` leaves out an experimental entry, so no picker of the Mac,
  the iPhone and iPad, or the Windows and Linux app offers it for a new pane. A custom entry of
  `harnesses.json` is always listed. Checking a harness is changing its marker (and
  `SNAPSHOT_DIGEST`) in the same pull request that adds it to the e2e run.
- **Panes already bound to a hidden harness keep working.** Everything that reads a harness by its id
  (`model.list`, opening or restoring a pane, the launch of an existing session, a task preset that
  names it) reads the whole catalogue, not the listed part. A pane bound to
  one runs and restores as before; its header names it from the pane's own name until the person
  switches away, and then it is no longer in the picker. Nothing is migrated or closed.
- **Not a block.** A request that names an experimental id (the command line's `pane create
  --agent`, a task preset) still starts it. The marker decides what is offered, not what an
  expert may ask for.
