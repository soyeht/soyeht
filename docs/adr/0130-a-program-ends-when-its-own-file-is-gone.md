# ADR 0130: The engine and the supervisor end when their own file is gone

- Status: accepted
- Date: 2026-10-31

## Context

ADR 0120 registers the engine and the supervisor as launchd agents that name the programs inside
`Soyeht Next.app`. Its amendment made the agent's `KeepAlive` a `PathState` on the program, so
launchd does not restart a program whose file is gone. That stops the restart loop, but not the
programs: a QA pass on the Mac (reading the code and the ADR, not running an app) found that
dragging the app to the Trash leaves the two programs running, because a process whose file is
removed or moved keeps running from the file's inode. The person has deleted Soyeht and Soyeht
still runs, holds its terminals and its ports, and stays in Login Items; it ends only at the next
logout or restart. The registration file stays in `~/Library/LaunchAgents`, inert.

The question was how the removal should take the running programs with it.

## Options

**(a) Register through `SMAppService`**, with the two agent property lists inside the bundle
(`Contents/Library/LaunchAgents`, `BundleProgram`), so that macOS ties the items to the app and
drops them when the app is deleted. ADR 0120 already weighed this and kept the seam, for two
reasons that still hold: a bundled list is static and signed, while the data directory and the log
path differ per person and launchd does not expand them (so the two security-sensitive programs
would have to find their own directory and open their own log, and the Mac would differ from Linux
and Windows); and the seam would have two writers of one label, which `soyeht service`,
`soyeht uninstall`, `soyeht diagnostics` and the Dev installer all read. It also needs a Developer
ID, a graphical session and the person's approval in Login Items, and cannot be exercised on the
machines this change was made on. Even then it would not decide the question here. What macOS
does to a program that is *already running* when the system notices the deleted app is not
documented, and was not tested; the registration would go and the processes might not.

**(b) The programs end by themselves when the file they were started from is gone**, with the
registration left in place and inert (`PathState`, ADR 0120). Nothing about registration changes.
It works the same on every system and for every way a program can lose its file (the Trash, a
package removal, an ejected disk image, a deleted build folder), needs no signature or approval,
and is testable as plain code.

## Decision

Option (b). `soyeht_platform::program::on_gone` reads the path the process was started from
(`std::env::current_exe()`, once, at start) and starts a thread that looks at it every 10
seconds. The program is **gone** when the system says there is nothing at the path three looks in
a row (about 30 seconds); a look that finds the file resets the count, and a file that cannot be
read for another reason is taken to be there. The debounce is the point: an update that replaces
the bundle, or a build that writes the program again, takes seconds and must not end a supervisor
that holds the person's terminals.

- **The engine** adds this to `soyeht_platform::shutdown::requested`, the one place that already
  says "stop": the engine closes its clients, removes its socket and returns, exit code 0. Nothing
  new is done at the call site.
- **The supervisor** calls `on_gone` once its endpoint is bound. It logs `ptyd.program_gone`,
  removes its endpoint and exits with code 0. Its terminals end with it, as they do at any stop of
  the supervisor: the app they ran in is gone.

Both exit with 0, so launchd does not treat the exit as a crash, and `PathState` keeps launchd
from starting the program again while the file is missing. If the app comes back (dragged out of
the Trash, installed again) launchd starts the program by itself, as ADR 0120 amended says, and
the app's "Start Soyeht" offer is the recovery that does not depend on that.

A copy that cannot read its own path (`current_exe` fails) runs as before; the watch is a
safeguard and its failure is logged, never a reason to stop.

## Consequences

- Deleting `Soyeht Next.app` ends the engine and the supervisor within about 30 seconds on the Mac.
  What is left is the registration file, which does nothing; `soyeht uninstall` before deleting
  the app still removes it, and the data directory is kept either way (ADR 0118).
- Moving the app to another folder while it runs now also ends the programs (the old path is
  gone) and the terminals with them. The app registers again from its new place at its next
  launch (ADR 0120, "A moved app does not leave a stale path"), so they start there. Before this,
  a moved app left programs running from the old place until the next logout.
- Linux and Windows get the same behaviour from the same code. A systemd unit still restarts a
  program whose file is gone (`Restart=always`) and the start fails each second; that was already
  so, and is the business of the Linux package (ADR 0121), not of this record.
- The same applies to a Dev build run from a build folder that is deleted while it runs.

## Not verified

- **Nothing was run against launchd or an app in the Trash.** The check is unit-tested (the
  counting, a real file deleted under a fast watch, a path that cannot be read) and the programs
  were not run through a Trash. The Mac behaviour the decision rests on, that a running process
  survives the removal of its file and that `current_exe` still names the old path afterwards, is
  the documented behaviour of the system, not something this record observed.
- The 30 seconds are a choice, not a measurement of how long an update takes to replace a bundle.

### Manual check (a Mac that is not the owner's working one)

Install the production app, say yes to the background start, then drag the app to the Trash. Within
about a minute `pgrep -f "Soyeht Next.app/Contents/MacOS/soyeht-"` finds nothing and
`launchctl print gui/$(id -u)/com.soyeht.next.engine` shows the agent loaded and not running.
Drag the app back: the engine starts without any command.
