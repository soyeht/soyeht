# ADR 0137: A Claude pane runs the Claude Code the person has

- Status: accepted. Extends ADR 0079 (external commands off the Core lock), ADR 0102 (the launch
  path) and ADR 0126 (process starts off the Core lock).
- Date: 2026-11-03

## Context

The Claude adapter drives a Claude Code binary that ships inside its own package. That binary is
as old as the adapter pin. Measured on the owner's machine with the adapter at 0.77.0: the bundled
Claude Code (2.1.270) answered `400 ... does not support this model; version 2.1.280 or newer is
required` for the current model, so panes stayed on an older default, and on the same broad task
(audit four independent areas) it did the work in series where the person's own Claude Code
(2.1.312, current model) started four subagents without being asked.

A Soyeht pane has to behave as the person's own Claude Code does: the same version, the same
models, the same automatic subagents.

## Decision

**The adapter is told to drive the person's `claude`.** The adapter honours
`CLAUDE_CODE_EXECUTABLE` for the CLI it starts. When the person has a `claude` on the launch
`PATH` (the `PATH` their login shell has, `agents/environment`; no second resolver) and it reports
a version of at least 2.1.283, the engine sets that variable in the environment of the adapter
process. Otherwise it sets nothing, and the adapter starts the CLI of its package.

- **One table.** `agents/user_cli.rs` holds, for each CLI an adapter can drive, the built-in
  harness whose adapter honours the variable, the program (`claude`; `claude.exe` on Windows, which
  starts a `.exe` and not the `.cmd` shim an npm install leaves), the variable and the minimum
  version. The adapter package is not typed there: it is read from the registry snapshot's pinned
  entry of that harness, so a bump of the pin cannot leave the engine looking for the old name.
- **The minimum is 2.1.283, decided against adapter 0.100.0.** That adapter ships with SDK 0.3.316,
  whose bundled CLI is 2.1.316, and is written against CLIs from 2.1.206 on. 2.1.280 is the first
  CLI that accepts the current models; the adapter's own baseline for them is 2.1.283: from it
  `opus` and `default` are the natively 1M-token models its context handling expects, and its
  model switch takes about 0.5 s where older CLIs take about 15 s. A CLI between the two would
  start but run worse than the bundled one. A CLI newer than the bundled one is fine. The value
  is not client-visible, so it is an engine constant. It is tied to the pin: a test fails when
  the snapshot pins an adapter other than the one the minimum was decided against.
- **The version is asked of the program.** `claude --version` runs through `soyeht_process::run`
  with the launch `PATH` as its `PATH` (an npm install starts through `env node`). It runs where
  a process is sent to the supervisor, off the Core lock (ADR 0126): `Launch::send` for a pane's
  process, and the job that starts the candidate of a harness switch. Both go through one helper,
  `user_cli::prepare` and `Given::send`, so every spawn of an adapter process is covered by one
  rule. The answer is kept for the program's file (canonical path, size, modification time), so a
  program that did not change is not asked again and one that updated in place is asked once more.
  A program that did not answer (a timeout on a busy host) is asked again after 30 s, and at once by the first pane when it was the start-up probe that went unanswered. A suffix
  such as `-beta.1` or `+build` counts as its release; anything that is not
  `major.minor.patch` is no version. The running engine also asks once at start, in the
  background, so the first pane does not wait; a unit test that opens a Core never does.
- **The adapter gets the person's path, not the file of one version.** The variable names the
  entry the search path has (`~/.local/bin/claude`), which an update keeps right when it replaces
  the file it links to. The canonical path is only the key of the cache.
- **A launch that sets the variable itself keeps its choice.** A custom harness entry or a test that
  names its own CLI is not overridden. Only the adapter process (`PipesAcp`) is given the
  variable; a sign-in terminal is not.
- **The supervisor lets it through, for an adapter only.** `soyeht-ptyd` takes from the engine a short
  list of variables (`PATH`, `LANG`, ...) and refuses any other. `CLAUDE_CODE_EXECUTABLE` joins the list
  for `PipesAcp` processes alone. A supervisor that outlived an update does not know it and answers the
  spawn with `operation_failed: environment variable is not allowed`; the engine then starts the
  adapter without the variable (the bundled CLI) and logs `engine.agent_cli_refused`, instead of
  leaving every Claude pane unable to start until the supervisor is restarted. Any other refusal is
  not retried.
- **The choice is logged, as a kind.** `engine.agent_cli_chosen` carries `choice` (`own` or
  `bundled`) and, for `bundled`, `why` (`not_installed`, `too_old`, `version_unknown`), at the
  first spawn and at each change. No path is logged.

**The adapter pin moves to 0.100.0**, so the CLI it bundles (2.1.316) is also new enough for
current models when the person has none. Only the Claude entry of the registry snapshot changes; it
is a hand edit of one entry, not a newer upstream registry, and the snapshot's `sourceSha256` still
names the registry the other entries come from.

**Subagents.** The engine still declares the AIR `nativeSubagentSessions` capability next to the
draft `subagents`, and the 0.100 adapter, which gives the AIR form to a client that carries it, reports
`subagent_spawned` and `subagent_state_update`, so the nested rows of contract 04 are unchanged. Two
differences were measured and are handled. The adapter reports a subagent's model as the `model`
option of the subagent's own session and no longer on the spawning call, so the engine reads that
option into the row (it is the one part of a subagent's configuration it keeps). A background
subagent keeps reporting after the parent turn ends; its completion notification is a further
generation of it, shown as a second row (contract 04).

## Consequences

- A person with an older Claude Code than 2.1.283 runs the bundled one and is not told; the log says
  why. Updating their Claude Code takes effect at the next spawn.
- The model catalogue of a Claude pane now follows the person's Claude Code, not the pin.
- The Linux and Windows paths are the same code with their own program name; they are not run in
  the merge gate's e2e.
