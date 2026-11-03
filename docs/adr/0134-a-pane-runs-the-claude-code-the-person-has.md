# ADR 0134 · A Claude pane runs the Claude Code the person has

Date: 2026-11-03
Status: accepted. Extends ADR 0079 (external commands off the Core lock) and ADR 0102 (the
launch path).

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
a version of at least 2.1.280, the engine sets that variable in the environment of the adapter
process. Otherwise it sets nothing, and the adapter starts the CLI of its package.

- **One constant.** `agents/user_cli.rs` holds the program (`claude`, `claude.exe` on Windows, which
  starts a `.exe` and not the `.cmd` shim an npm install leaves), the variable, the adapter
  package that honours it and the minimum version. The minimum is not a client-visible value. It
  is the first version that accepts the current models; the CLI inside the pinned adapter is
  newer than it.
- **The version is asked of the program.** `claude --version` runs through
  `soyeht_process::run`, from `Launch::send`, the one place where every process the engine asks
  the supervisor for passes off the Core lock (ADR 0126). The answer is kept for the program's
  path, size and modification time, so a program that did not change is not asked again and one
  that updated in place is asked once more. The engine also asks once at start, in the
  background, so the first pane does not wait.
- **A launch that sets the variable keeps its choice.** A custom harness entry or a test that
  names its own CLI is not overridden. Only the adapter process (`PipesAcp`) is given the
  variable; a sign-in terminal is not.
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

- A person with an older Claude Code than 2.1.280 runs the bundled one and is not told; the log says
  why. Updating their Claude Code takes effect at the next spawn.
- The model catalogue of a Claude pane now follows the person's Claude Code, not the pin.
- The Linux and Windows paths are the same code with their own program name; they are not run in
  the merge gate's e2e.
