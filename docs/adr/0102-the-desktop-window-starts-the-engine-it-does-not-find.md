# ADR 0102: The desktop window starts the engine it does not find

- Status: accepted
- Date: 2026-10-22

This is numbered 0102 because 0098 to 0101 are used or claimed by open pull requests (#599
and #600) when it was written.

## Context

On macOS the engine and the process supervisor are launch agents (`scripts/install_dev.py`), so the Mac app finds them running. Linux and Windows have no such service yet. A
person who opens the desktop client for the first time there sees "The engine is not running"
and a README that says to start it by hand. The complete answer is a service seam (a launch agent,
a systemd user unit, a scheduled task, and a `soyeht service` command) that depends on a
production profile that is not decided. Until it exists, a first run must not be a dead end.

## Decision

**The window starts what it does not find.** When the desktop client cannot connect because
nothing listens at the engine endpoint, it starts `soyeht-ptyd` (when the supervisor does not
answer) and `soyeht-engine`, waits for the engine to answer and connects. The programs are in
the folder of the window's own executable, where a release builds them side by side. The window
shows "Starting the engine…" in the place where it shows "Connecting to the engine…".
When a program is missing or the start fails, the window shows a problem line that names the
program and no path, and the log keeps only the kind of the failure (`desktop.engine_start_failed`,
ADR 0088).

**Same data directory, same profile.** Both programs get `--data-dir` with the directory the
window resolved from the Dev profile, so they listen where the window looks. Their standard
output and error are appended to `logs/ptyd.log` and `logs/engine.log` in that directory, the
file the Mac launch agents already use, and the engine bounds its own file as it does there.

**Detached, in `soyeht-platform`.** `process::detached::spawn_detached` is the one place that
knows how to start a program that outlives its parent, with the same public shape on every
system. On Unix the child leads a session of its own. On Windows it has a hidden console (and, since
ADR 0108, no process group of its own, which would make every terminal ignore Ctrl+C) and leaves
the parent's job when the job allows it (a launcher that ends its job otherwise ends everything in
it). Closing the window leaves the engine and every terminal
running, and the next window connects to them.

**One engine.** The engine and the supervisor each keep a lock in the data directory, so a second
copy cannot run. The window adds a lock of its own (`desktop-start.lock`) around the decision, and
checks the endpoint again once it holds the lock: two windows opened together start one engine,
and the second waits for the first. A window that finds an engine does nothing.

**An explicit engine is left alone.** When `SOYEHT_ENGINE_SOCKET` names an endpoint, the window
starts nothing, whether or not the endpoint answers.

**Once per absence.** A start is tried when the connection fails because nothing listens, and not
again until a connection has been made, so an engine that stops later is started again but a
failure is not repeated at every retry. What "nothing listens" means on Windows, and who starts
the supervisor again when it alone is lost, are in ADR 0108.

## Limits

- The engine listens on a fixed loopback port (the profile's `tcp_port`, ADR 0112), so a second engine for another
  data directory stops at once; the window then says that the engine stopped right after it started.
- On Windows a launcher that runs the window in a job that kills its processes and does not allow
  breakaway (an automation agent's command, for example) ends the engine with the window. Starting
  the window from the shell, the Start menu or Explorer does not.

## What stays as it is

The Mac app and its launch agents do not change. The Mac path and this one never meet: the Mac
app does not start programs.

## How this fits the service seam

This step is the interim that the service seam replaces. When the seam exists, an install
registers the engine and the supervisor with the system, the window asks the seam to start the
service instead of starting the programs, and `spawn_detached` and the window's lock go away
with the code that calls them. Nothing else depends on this: the endpoint, the data directory and
the handshake are the ones the service uses.

## Consequences

- A first run on Linux and Windows works from one folder that holds the desktop, the engine, the
  supervisor and `soyeht-mcp` (`docs/platforms.md`, "Release builds").
- A person who quits the engine on purpose finds it started again by the next window.
- The programs run as the user who opened the window and see that window's environment, which
  is the environment a harness will inherit. A service will start them from the system's.
