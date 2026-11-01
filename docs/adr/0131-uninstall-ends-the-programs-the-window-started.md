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

**`soyeht_platform::process::terminate(pid)` asks it to end.** On Unix it sends `SIGTERM`. The
engine answers it by stopping cleanly (`shutdown::requested`); the supervisor has no handler, so it
ends at once with its terminals, and leaves its socket file behind (the next start replaces it).
On Windows it ends the process at once (`TerminateProcess`), and what it started goes with it,
because each child runs in a job that closes with it. It refuses 0, 1 and its own process, and
says `NotFound` for a process that is gone.

**`Manager::uninstall` ends what still runs.** After it unregisters a component (the engine
first), if that component's lock is still held it reads the process, asks it to end through the
`Commands` seam (so a test ends nothing), logs `service.program_ended` with the component, and
waits up to ten seconds for the lock to be free before it goes on to the supervisor. Every
component is tried whatever happens to the one before: a program that cannot be asked (another
account's process, one that is exiting) or that does not end, or that holds the lock with no id
(a program of an older build, which wrote none), is reported once both were tried, as the system's
error or `StillRunning`. The services are unregistered by then, and the message and the data
deletion behave as they did. So `--delete-data` works when the programs end, and still refuses,
with the same message, when they do not.

**An open window does not start them again.** The window starts an engine when it finds none
(ADR 0102), and an engine ended by an uninstall is none. So the uninstall first leaves a file,
`uninstalled`, in the data directory (`fs::mark_uninstalled`), before anything is ended, and
`autostart::ensure` starts nothing when that file is newer than the window's own start
(`fs::uninstalled_since`), saying so in the window ("Soyeht was uninstalled, so this window does not
start the engine again"). A window opened after the uninstall began later than the file and starts
afresh. `--delete-data` removes the file with the directory, so a window that has had an engine
also treats a directory that is gone as an uninstall; a first run, which has no directory yet, does
not. `service install` takes the file away, since a registration written again is a new start.

`service uninstall`, the low-level command, shares `Manager::uninstall` and so ends them too, and
leaves the file; its contract line said "stops". `service stop` is unchanged: it stops what is
registered.

## Consequences

- Stopping the supervisor ends the person's terminals, as contract 13 section 7 already said.
- An open window is not closed. It shows that it was uninstalled and stays until the person closes
  it; it connects again if an engine is started by other means.
- A program of a build before this one holds its lock with an empty record. `soyeht uninstall`
  of the new build then reports `StillRunning` for it, as it did for every unregistered program
  before, until the person quits it; the next start writes the record.
- The record of the process is trusted while the lock is held, and the lock proves that its writer
  lives only for a build that writes it. A build before this one takes the lock and writes nothing,
  so if a newer build had written an id and ended, and an older build then took the lock, the old
  id is still in the file while the lock is held, and may by then name another program of the
  same account. The record carries no start time or name to check against, because there is no
  portable way to read them from the id alone. The risk is a stale id surviving a downgrade; the
  two reads do not catch it. A program that did not end is reported, never ignored.
- Windows ends the engine without a clean stop. Its database is the engine's own concern at
  every start (it is written transactionally), and the agents and terminals die with the job.
  Not verified on a Windows machine: the code is compiled for the Windows target and covered by
  the shared tests, not run there.
- Not changed: the Mac Trash case (a program noticing that its own program is gone, ADR 0130) and
  the Mac app, which registers its services and so never had an unregistered program to end.
