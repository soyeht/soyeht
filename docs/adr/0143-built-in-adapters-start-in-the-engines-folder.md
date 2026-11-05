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
folder is the engine's: owner-only, made before each start (so one that was removed is made again),
empty. This holds for the agent process, the candidate process of a harness switch and the
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
