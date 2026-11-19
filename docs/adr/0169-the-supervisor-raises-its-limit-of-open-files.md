# ADR 0169: The supervisor raises its limit of open files

- Status: accepted
- Date: 2026-11-19
- Builds on ADR 0086 (the operating-system seams) and ADR 0088 (the structured log).

## Context

A QA run on a Mac at the owner's scale (60 terminals and 20 agents) found the supervisor out of
descriptors at about 75 panes: it held 251 against the soft limit of 256 that launchd gives an
agent (about 3.2 per pane). Five agent panes ended dead with "The pane's process could not be
started", and the engine logged `ptyd_unavailable`. Nothing raised the limit, and a run out of
descriptors at `accept` ended the supervisor, and every terminal it held, as any listener error
does.

## Decision

1. **One seam, `soyeht_platform::descriptors`.** `raise_and_log` raises the soft limit of open files
   of the calling process to the hard limit, up to `WANTED` (65,536), and writes one log line with
   the limit it ended with. On macOS the number is also bounded by `kern.maxfilesperproc`, which
   the system keeps apart from the limit (a soft limit above it fails with `EINVAL` even under an
   unlimited hard limit); a refusal is tried again once with 10,240, unless that would lower the
   limit. It never lowers a limit. Windows has no such limit to raise (a process holds millions of
   handles, and the C runtime's `_setmaxstdio` governs `FILE` streams that Rust does not use), so
   there it changes nothing. Only `soyeht-ptyd` calls it, first thing.
2. **The engine does not raise its limit.** It holds a few dozen descriptors however many panes
   there are (26 with 300 terminals, against 910 in the supervisor). A restore of the limit in the
   children of a program that raised would also move every command of `soyeht_process::run` from
   `posix_spawn` to fork and exec, which costs tens of milliseconds once the engine's memory is
   large, under the spawn lock.
3. **A started terminal gets the limit the supervisor started with.** A raised soft limit is
   inherited, and programs that close every descriptor up to it, or use `select`, are slow or wrong
   with tens of thousands. `restore_in_child` puts the limit the supervisor began with back in the
   child before the exec, for terminals and for the guardian of an agent, which the supervisor forks
   anyway. The service files carry no limit of open files: a program started with the raised limit
   would have nothing to raise, and so nothing to restore.
4. **Running out is said, and survived.** A spawn the supervisor cannot make for lack of a
   descriptor answers `too_many_open_files` (the supervisor's own kind, as `spawn_refused` is), which
   the engine presents as the new protocol error code `too_many_open_files` (`ErrorKind` is an
   open enum, ADR 0161); a pane that never starts for it is stopped with a reason that names the
   cause, not the generic one. A listener of the local endpoint that meets a lack of descriptors,
   buffers or memory waits and asks again, and never ends; one rule
   (`soyeht_platform::net::exhausts_resources`) serves it and the engine's accept loops.

## Consequences

- A supervisor with 65,536 descriptors has room for about 20,000 panes; the supervisor's retention
  of 8 MiB per terminal bounds a flood first.
- A test that starts the supervisor and the engine under a soft limit of 256 and opens 300 terminals
  keeps this, and a shell under the supervisor is checked to have the limit the supervisor started
  with.
