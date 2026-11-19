# ADR 0168: The supervisor raises its limit of open files

- Status: accepted
- Date: 2026-11-19
- Builds on ADR 0114 (the service seam), ADR 0086 (the operating-system seams) and ADR 0088 (the
  structured log).

## Context

A QA run on a Mac at the owner's scale (60 terminals and 20 agents) found the supervisor out of
descriptors at about 75 panes: it held 251 against the soft limit of 256 that launchd gives an
agent (about 3.2 per pane). Five agent panes ended dead with "The pane's process could not be
started", and the engine logged `ptyd_unavailable`. Nothing raised the limit, the launchd agent
carried no `SoftResourceLimits`, and a run out of descriptors at `accept` ended the supervisor,
and every terminal it held, as any listener error does.

## Decision

1. **One seam, `soyeht_platform::descriptors`.** `raise_and_log` raises the soft limit of open files
   of the calling process to the hard limit, up to `WANTED` (65,536), and writes one log line with
   the limit it ended with. On macOS the number is also bounded by `kern.maxfilesperproc`, which
   the system keeps apart from the limit (a soft limit above it fails with `EINVAL` even under an
   unlimited hard limit); a refusal is tried again once with 10,240. It never lowers a limit.
   Windows has no such limit to raise (a process holds millions of handles, and the C runtime's
   `_setmaxstdio` governs `FILE` streams that Rust does not use), so there it changes nothing.
   `soyeht-ptyd` and `soyeht-engine` call it first thing.
2. **A started program gets the limit it would have had.** A raised soft limit is inherited, and
   programs that close every descriptor up to it, or use `select`, are slow or wrong with tens of
   thousands. `restore_in_child` puts the limit the process began with back in the child before the
   exec. The supervisor does it for terminals and for the guardian of an agent, `soyeht-process`
   for every command the engine runs, and `process::detached` for the programs one starts.
3. **The service files ask for the same number**, from the same constant: `SoftResourceLimits` and
   `HardResourceLimits` (launchd) and `LimitNOFILE=` (systemd), so a start has it before any code
   runs. Task Scheduler has no equivalent.
4. **Running out is said, and survived.** A spawn the supervisor cannot make for lack of a
   descriptor answers `too_many_open_files` (the supervisor's own kind, as `spawn_refused` is), which
   the engine presents as the new protocol error code `too_many_open_files` (`ErrorKind` is an
   open enum, ADR 0161); a pane that never starts for it is stopped with a reason that names the
   cause, not the generic one. A listener of the local endpoint that meets `EMFILE` or `ENFILE`
   waits and asks again, and never ends.

## Consequences

- With the service files, a supervisor has about 20,000 panes of room; the supervisor's retention of
  8 MiB per terminal is what bounds a flood first.
- A test that starts the supervisor and the engine under a soft limit of 256 and opens 300 terminals
  keeps this.
