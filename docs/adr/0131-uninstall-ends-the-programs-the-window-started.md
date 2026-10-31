# ADR 0131: Uninstall ends the programs the window started

- Status: accepted
- Date: 2026-10-31

## Context

Contract 13 (section 7) says `soyeht uninstall` "stops the engine and the supervisor of the
profile". It stopped only what a service registration covered: the launchd agent, the systemd
unit or the scheduled task. On Linux and Windows the first run starts the engine and the
supervisor from the window, detached and unregistered (ADR 0102), and a person who never ran
`soyeht service install` has no registration. For that person `soyeht uninstall` unregistered
nothing, stopped nothing, and the programs went on running, from a folder the AppImage's
`uninstall` then deleted. `soyeht uninstall --delete-data` refused ("quit the app or program
that runs it") although nothing in the window or the command line could quit a detached engine,
so the only way out was to kill it by hand.

Seen on Ubuntu 26.04 arm64: after `Soyeht.AppImage soyeht uninstall` the engine, the supervisor,
an agent and the MCP server it had started still ran.

## Decision

**A program records its process id in its own lock.** `fs::lock_singleton` (the lock the engine,
the supervisor and the window's start take in the data directory) writes the id of the process
over the contents of the lock file once it holds the lock. The lock file already existed, is
owner-only, and its contents were left readable on purpose (the Windows lock is a byte range far
beyond them). No new file, no new wire method and no change to the engine or the supervisor.

**The record is trusted only while the lock is held.** `fs::singleton_process(directory, name)`
returns the id only when the lock is held, which proves that its writer is alive, so the id of a
program that ended is never returned. A program that has taken the lock and not yet written its
id would show the previous holder's id, which may now name another program, so the id is read
twice, 50 ms apart, with the lock held both times, and only an id both reads agree on is
returned. The caller that signals a process group is told never to use an id from a previous run
(`process::signal_group`); this is the other case, an id that the lock vouches for.

**`soyeht_platform::process::terminate(pid)` asks it to end.** On Unix it sends `SIGTERM`, which
the engine and the supervisor already treat as a request to stop cleanly (`shutdown::requested`).
On Windows it ends the process at once (`TerminateProcess`), and what it started goes with it,
because each child runs in a job that closes with it. It refuses 0, 1 and its own process, and
says `NotFound` for a process that is gone.

**`Manager::uninstall` ends what still runs.** After it unregisters a component (the engine
first), if that component's lock is still held it reads the process, asks it to end through the
`Commands` seam (so a test ends nothing), and waits up to ten seconds for the lock to be free
before it goes on to the supervisor. A program that does not end, or that holds the lock with no
id (a program of an older build, which wrote none), is `StillRunning` once every component has
been tried: the services are unregistered by then, and the message and the data deletion behave
as they did. So `--delete-data` works when the programs end, and still refuses, with the same
message, when they do not.

`service uninstall`, the low-level command, shares `Manager::uninstall` and so ends them too; its
contract line said "stops". `service stop` is unchanged: it stops what is registered.

## Consequences

- Stopping the supervisor ends the person's terminals, as contract 13 section 7 already said.
- The window is not told. An open window sees its engine go and reconnects until it is closed.
- A program of a build before this one holds its lock with an empty record. `soyeht uninstall`
  of the new build then reports `StillRunning` for it, as it did for every unregistered program
  before, until the person quits it; the next start writes the record.
- Windows ends the engine without a clean stop. Its database is the engine's own concern at
  every start (it is written transactionally), and the agents and terminals die with the job.
  Not verified on a Windows machine: the code is compiled for the Windows target and covered by
  the shared tests, not run there.
- Not changed: the Mac Trash case (a program noticing that its own program is gone, ADR 0130) and
  the Mac app, which registers its services and so never had an unregistered program to end.
