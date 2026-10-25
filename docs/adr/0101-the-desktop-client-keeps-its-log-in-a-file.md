# ADR 0101: The desktop client keeps its log in a file

- Status: accepted
- Date: 2026-10-20

## Context

`soyeht_platform::log` writes every line to standard error (ADR 0088). A release desktop client
on Windows is a GUI-subsystem program with no console, and a Linux client started from a menu
has no terminal, so every line was lost, including the advice about a missing graphics driver
and the failure to open the window. The engine already bounds its own log file (ADR 0082), but
that bound lived in the engine crate, which the desktop may not depend on.

## Decision

- `RotatingFile` and its bound (16 MiB, four rotated files, a line cut at 64 KiB, rotation only
  between lines, owner-only creation) move to `soyeht_platform::log::rotating`. The engine keeps
  only what is specific to it: replacing standard error with a pipe and copying it (ADR 0082).
- `soyeht_platform::log::to_file(path)` makes every log line also go to a rotating file, from
  then on. It creates the file's directory owner-only. Standard error stays as it was.
- The desktop client calls it first thing, with `logs/desktop.log` in the Dev profile directory
  (`soyeht_client::profile::log_file`), beside the engine's `logs/engine.log`, and logs the path
  once (`desktop.log_file`). A client that cannot open the file logs the I/O kind to standard
  error and goes on.
- The advice the client prints for a person (`Notice`) is also a `desktop.notice` line, so it
  reaches the file. A failure that stops the client before it has a window (the profile
  directory cannot be found, the window cannot open) logs only a fixed kind (`desktop.fatal`),
  never the error's text, which can carry a path. The person sees the full text, with the error,
  on the terminal and in a native message box through `soyeht_platform::dialog::error`, which
  does something on Windows only. The missing engine
  already shows in the window (the stage hint). The software-renderer note shows in a standing
  notice line (`State::notice`) that a connection does not clear, unlike the problem line.
  The window shows one line at a time (a failure first, as the Mac shows one failure at a time),
  and dismissing the software-renderer note is kept for good in the device's own choices file
  (`client-preferences.json`, beside the engine socket).
- Every failure the window tells the person goes through `State::report`, which also logs
  `desktop.problem` with a bounded `kind` (what could not be done, plus the cause's own kind
  when it has one, such as `request_failed.not_found`). The text shown to the person is never
  logged: it can carry a path or an engine's own words.
- The `desktop.log_file` line also carries the build id (`0.1.0+<commit>`).

## Consequences

- One rotation rule serves the engine and the desktop.
- A failure before the window is seen on Windows, and every client leaves its log on disk. With
  no home directory there is no profile directory, so that one failure has no file.
