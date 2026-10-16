# ADR 0088: One platform log API, and failure streaks

- Status: accepted
- Date: 2026-10-15

## Context

The engine wrote its log lines three ways: `soyeht_platform::log::event`, an engine helper
that took an `Error`, and dozens of hand-assembled `eprintln!("{}", json!({...}))` calls.
The platform doc said no log line is assembled by hand, and the code disagreed. The hand
built lines escaped values their own way, put the error text where a kind belonged (and
error text can carry a path), and each one chose for itself whether to log a failure once,
at every power of two, or on every tick. The desktop printed plain `soyeht: ...` lines.
Errors that a `match` arm dropped with `Err(_) =>` were not counted by the swallowed-error
gate at all.

## Decision

**One log API, in `soyeht-platform`.** `log::event(name, fields)` writes a line.
`log::failure(name, &error)` and `log::failure_with(name, fields, &error)` add a `kind` field
taken from the `Failure` trait, whose label is bounded: a protocol error kind, an I/O error
kind, a fixed word. It is never message text. `log::Line` is a line built now and written
later, for a record made under the Core lock or read by a test. A line is written with
`writeln!`, and a closed standard error is ignored, so logging cannot panic. The engine's
`Error` implements `Failure` (an I/O error logs `io:` and its kind, since every one of them
has the protocol kind `internal_error`), and so does `soyeht-mcp`'s.

**One failure streak, in the engine.** `Streak` keeps the consecutive failures of one thing
that is retried or polled. It says when to log (the first failure and each change of kind;
`Streak::retrying` also at attempts 2, 4, 8, ...) and, for a retried thing, how long to wait.
The editor effects (ADR 0082), the tick, the background jobs, the usage schedule and the
tailnet lookup use it. The caller logs the recovery when a recovery is worth a line.

**A gate.** `scripts/check_logging.py` fails on `eprintln!` or `eprint!` in the `src` of
`engine` and `apps/desktop` outside `soyeht-platform/src/log.rs` and a binary's `main.rs`.
The allowed count is zero. A binary prints text for a person (the desktop's advice about a
missing graphics driver) from its `main.rs`.

**A fourth swallowed-error form.** `check_swallowed_errors.py` counts `Err(_) =>` arms with
the other forms and the baseline holds the number. An arm either logs, or propagates, or
keeps a one-line comment that says why the error carries nothing worth keeping.

## Consequences

- A log line has one shape and one escaping rule, and a lasting failure writes a few lines.
- Numbers in a line are text (`durationMs`, `attempts`).
- A new crate that needs to log depends on `soyeht-platform` only.
