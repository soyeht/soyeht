# ADR 0082: Bounded engine log and editor effect backoff

- Status: accepted
- Date: 2026-10-07

## Context

A developer Mac had an `engine.log` of 7.6 GB. Almost every line was
`engine.editor_effect_failed` for `action: watch`, `kind: internal_error`, on
a handful of file ids.

The cause was in two parts. First, the editor re-checks the disk of every open
file every 250 ms (`WATCH_INTERVAL_MS`). When the file or its workspace folder
no longer exists, `canonicalize` fails with `NotFound`, which the engine
reports as `internal_error`. Nothing changed after that failure, so every
following tick failed the same way, forever, and each failure wrote one line.
The autosave effect had the same shape on its own interval. Second, the launch
agent points the engine's stderr at `engine.log` and nothing ever trims that
file, so a loop like this one has no limit except the disk.

## Decision

**Backoff.** A failing editor effect (autosave or disk watch) waits twice as
long after each consecutive failure, up to 60 s. The attempt count is kept per
file and per effect in memory. A failure is logged only when the attempt
count is a power of two (1, 2, 4, 8, ...), with `attempts`, `retryInMs` and a
`cause` (the I/O error kind, never a path). When the effect works again the
engine logs one `engine.editor_effect_recovered` with the attempts it took, and
the count resets. A file that comes back (a branch switch, a moved folder) is
picked up on the next attempt, so there is no permanent give-up state to leave
behind. A day of failure logs about eleven lines per file instead of
350,000. The policy is the small `backoff::Backoff` type (base, cap and
attempts; `fail`, `next_delay_ms`, `should_log`, `succeed`), so other retry
loops can move onto it later; this change does not touch them. Log records
carry only the class of an error (`kind`, and the I/O error kind), never its
text, which can hold a path.

**Rotation.** When stderr is a regular file, the engine replaces it with a
pipe and a thread copies the lines into `RotatingFile` at the same path. The
file rotates before a line that would pass 16 MiB, keeps four rotated files
(`engine.log.1` to `engine.log.4`), rotates only between lines so every record
stays a whole JSON line, and cuts any single line at 64 KiB. The most disk it
can use is about 80 MiB. A log that is already over the cap when the engine
starts is rotated at once, so it stops growing and ages out after four more
rotations. Panic messages and other stderr text go through the same pipe. When
stderr is a terminal or a pipe nothing changes. The launch agent files need no
change.

## Consequences

- One failing loop can no longer fill the disk, whether or not the code that
  loops has backoff of its own.
- A failure that lasts for hours is visible as a few lines with growing
  `attempts`, not as a flood.
- The copy thread is bounded by a short grace wait when the engine shuts
  down; lines still in the pipe after that are lost.
- `ptyd.log` is not covered here. The supervisor keeps no editor loops and
  writes a few lines per start.
