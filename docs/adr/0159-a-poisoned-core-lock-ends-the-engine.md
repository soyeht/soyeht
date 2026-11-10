# ADR 0159: A poisoned Core lock ends the engine, and the service manager starts a new one

- Status: accepted. Extends ADR 0079 (the Core lock), ADR 0114 (the service seam) and ADR 0127 (crash reports).
- Date: 2026-11-10

## Context

The engine keeps all its state behind one lock, the Core lock (`core_lock.rs`). When a thread
panics while it holds the guard, Rust poisons the mutex. The engine turned that into
`Error::EngineStatePoisoned` for the caller and went on: the tick logged the failure once and kept
looping, every request failed for ever, and no service manager had a reason to act because the
process was alive. `engine/Cargo.toml` and `docs/platforms.md` said the engine "recovers a poisoned
lock", which holds for the small locks (`slow_holds`, `offload`) whose data a panic cannot leave
half changed, and not for Core.

Recovering the Core guard with `PoisonError::into_inner` would hide the question that matters:
a panic under the lock may have left the panes, the agents and the store's open transaction in a
state no code was written to meet. The terminals themselves do not depend on the engine: the
supervisor (`soyeht-ptyd`) holds the processes, and a new engine adopts them.

## Decision

**The first caller that finds the Core lock poisoned logs it and ends the process.**

- `core_lock::lock` (every Core lock goes through it, async callers through `core_lock::with`)
  writes one line, `engine.core_poisoned`, with the kind of the failure and no message text (ADR
  0088), then calls `Hooks::end_process(POISONED_EXIT_CODE)`. Later callers that arrive while the
  process is ending only fail their request.
- The exit code is 70 (`EX_SOFTWARE`), apart from the 1 of an engine that could not start
  (`engine.startup_failed`), so the log and the service manager's history tell them apart.
- `Hooks::end_process` is the single seam. Its default is `std::process::exit`, so the shipped
  engine (`NoHooks`) and the test program exit; a unit test overrides it to record the call
  (ADR 0091). It is the only place the engine exits from on purpose.
- The crash report of ADR 0127 for the panic itself is written by the panic hook, as before, with
  `fatal: no` since the panic is not on the main thread; the exit adds no second report.
  `panic = "unwind"` stays.

**What starts the new engine.** Every service definition restarts the engine after an end with a
failure code: launchd `KeepAlive` (any end, while the program exists), systemd `Restart=always`
(one second later), and the Windows task's `RestartOnFailure` (a minute later, up to 999 times;
the service host passes the program's exit code on whole, so 70 reaches Task Scheduler). Where no
service is registered, the desktop window starts a detached engine when it finds none (ADR 0102),
and the Mac app reconnects with its backoff. The new engine adopts the supervisor's terminals.

## Consequences

- One panic under the lock costs seconds of engine availability (a minute on Windows) instead of
  the whole session. Clients see the connection close and reconnect, as for any engine restart.
- The state half-changed by the panic is not kept: the store commits whole transactions, so the
  next engine opens the last committed state.
- A bug that panics under the lock on every start now restarts in a loop at the service manager's
  pace (one second; a minute on Windows) instead of staying up and broken. The log line and the
  crash reports name it.
- Not covered here: the places that can panic under the lock without `unwrap`, `expect` or
  `panic!` (map and slice indexing, narrowing casts). A follow-up denies `clippy::indexing_slicing`
  in the engine.
