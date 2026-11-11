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
start (so one that was removed is made again), and empty but for `installed/`, where the engine
puts the adapters it installs from a lockfile (see the amendment). This holds for the agent process, the candidate process of a harness switch and the
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

## Amendment (2026-11-07): the common adapters are installed once from a lockfile

### Context

A version pin names the adapter and nothing it depends on. `npx --yes <package>@<version>` resolves
every dependency from the registry when the adapter starts, and a package that depends on others
runs their install scripts. A substituted or compromised dependency, or a registry mirror, decides
which code a pane runs, at each start.

### Decision

- **The adapters of the two supported harnesses that are npm packages (`registry.claude-acp`,
  `registry.codex-acp`) are installed once from a lockfile the engine carries.** The lock
  (`agents/adapter_locks/<name>/package-lock.json`, `lockfileVersion` 3) holds the exact version and
  the SHA-512 integrity of every package of the tree. `npm ci --ignore-scripts` installs it into
  `<data directory>/adapters/installed/<name>-<version>-<lock digest>/`, in a staging folder that
  holds only the engine's manifest and lock, then moves it into place. npm refuses a package whose
  bytes differ from the lock; the installed adapter's own `package.json` must name the pinned
  version. The folder is named by the lock too, so a new lock never replaces a tree that an adapter
  of the older engine may still run.
  The other two supported harnesses (`registry.cursor`, `registry.opencode`) are programs the person
  installs and have no package to fetch.
- **A launch of the pinned `npx --yes <package>@<version>` is rewritten to `node <installed entry>`**
  where the start is sent, off the Core lock (`agents/adapter_install.rs`), for a pane, the
  candidate of a harness switch and a sign-in terminal alike. The first start installs it (a
  fetch of at most a few minutes); every later start finds the tree and runs no npm. An install
  that fails starts nothing and says why; there is no fallback to `npx`, which would be the
  unchecked path. Every send of an adapter process goes through `user_cli::prepare`, which does
  this rewrite, so a new way to start one cannot skip it.
- **The install never holds a shared worker or a request's deadline.** The four background workers
  also carry the terminal's flushes, and a request has the supervisor's deadline (30 s). A start
  that may install runs as a job on a thread of its own (`Background::run_alone`), and
  `pane.create` of such an agent answers at once with the pane starting, as it does while the
  supervisor is away. One install runs at a time, the starts that arrive wait for it and run its
  tree, and a failed install (offline) is remembered for a minute, so the panes restored after an
  upgrade do not each run `npm ci` again.
- **Each start checks the tree again.** The marker of the tree holds the digest of the lock and a
  digest of the entry script, the adapter's `package.json` and npm's own record of the tree
  (`node_modules/.package-lock.json`, the version and integrity of every package), taken when it
  was installed. A start compares them (a few files, off the Core lock); a tree that changed
  since, or whose entry is a link, is installed again. This notices an edit of what a start runs;
  it does not read every file of the tree, and it is not a defence against a writer with the
  person's own account, who can change the tree and its digest alike: it detects accidental
  edits and half-finished changes. The tree is an owner-only folder of that account.
- **An install is put in place without deleting what a running adapter holds.** A tree already
  there (the same lock, found changed or half made) is renamed aside and deleted after the new one
  is moved in, and renamed back if that fails. Staging folders and trees that could not be deleted
  are swept when the next install starts.
- **A test ties each lock to the registry snapshot**: the package and version of the snapshot's
  launch, the manifest, the root package of the lock, its `bin`, integrity on every package, and no
  package with an install script. A snapshot that moves to a new version without its lock fails
  the gate instead of going back to `npx`. Moving to a new version is: change the manifest, run
  `npm install --package-lock-only --ignore-scripts`, and read the diff of the lock.
- **The rest keep their package runner and say so before the first start.** The catalogue
  (`harness.list`) carries `package` (`name@version`) and `packageLocked` for a built-in adapter of
  a package runner. The pickers of the Mac, the iPhone, the iPad and the Windows and Linux app show,
  for an adapter that is not locked, "Starting it downloads and runs <package> and its
  dependencies from the package registry. Soyeht has not checked them.", and for a locked one that
  it is installed once from a lockfile Soyeht carries. The adapters that were not checked are not
  in the catalogue at all (see "Note for 0.1.0" below), so in 0.1.0 the notice is shown for no
  listed entry; it stays in the catalogue code for the first one that is listed unchecked.
- **The notice is enforced by the engine, not only shown.** The first start of an adapter that the
  runner downloads unchecked (a built-in `package`, `packageLocked` false) is refused with
  `adapter_not_acknowledged` unless the request that chooses the agent (`pane.create`,
  `pane.harness.switch`) carries `acknowledgedPackage` with that exact `name@version` and does not
  come through an agent binding. The acknowledgement is stored per package and version
  (`adapter_acknowledgements`), so a new version asks again.
  - **Who can acknowledge.** "A person" means "not an agent binding": the roles `human_local` and
    `human_device` can, and `agent` and `chief` get `forbidden_role`. A process of the same user
    on the local socket is `human_local`, as it is for every other full-access request; this
    check keeps an agent from acknowledging, it does not defend against that user.
  - **What consent is.** The acknowledgement is that the person started the agent from a client
    that shows the notice. A client sends the package its picker showed for every start of that
    agent, so the default harness and the shortcuts that start it count, not only a press on a
    button beside the notice.
  - **Every other path** (a task, a role, a restart, a recovery, the candidate of a switch,
    another client that sends nothing) finds the stored acknowledgement or is refused. The check
    runs where the agent is chosen and again where the process is sent. A pane that was already
    open when the engine was updated, and that runs such an adapter, stops at the next start with
    the package named in its reason (`Acknowledge <package> to start this agent ...`) instead of
    being retried; nothing is acknowledged for it afterwards on the person's behalf.
  - **The catalogue** keeps the notice (`registry_package_unverified`) until the package is
    acknowledged, also after a start that failed.
  - **A test** ties the check at launch to the catalogue for every entry of the registry snapshot,
    so a form of command line the check does not read (`-y`, `--package`) cannot slip past it.

### What this does not do

- The lock fixes what is installed, not whether it is good: it is as trustworthy as the version it was made at. A reviewer of
  a change to a lock reads its diff.
- `uvx` adapters (`fast-agent`, `minion-code`) have no lock yet.
- Trees of earlier locks stay in `installed/`. Pruning them is a follow-up: a tree may still be
  run by an adapter that the supervisor keeps alive across an engine update, so deleting one is
  safe only after asking the supervisor which processes run from it, which the install (it has no
  supervisor handle) cannot do cheaply.
- `npm` must be on the launch `PATH`, as `npx` was; a person's own registry configuration still
  applies to the fetch, and the integrity check still applies to what it returns.
- The adapter still drives the person's own CLI (ADR 0137), which is outside this install.

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
- **How this fits the acknowledgement of the amendment.** The two are separate gates on separate
  questions. The marker decides whether a picker *offers* an entry; the acknowledgement decides
  whether an unchecked package the runner downloads may *start*. Hiding an entry does not weaken
  the acknowledgement: a request that names an unlisted `npx`/`uvx` id by hand is still refused
  with `adapter_not_acknowledged` until it carries `acknowledgedPackage` with that exact
  `name@version`, and it is still refused for an agent binding (`forbidden_role`). Because no
  picker lists the entry, the notice that would show "downloads and runs <package>" is not on
  screen for it; a person who names the id or a custom entry is the one who sends the
  acknowledgement. The tests keep the two apart: `adapter_acknowledgement.rs` reaches the unchecked
  package by explicit id and asserts that `harness.list` does not list it before or after it is
  acknowledged, and `agents::catalog` tests the notice and the listing on their own.
