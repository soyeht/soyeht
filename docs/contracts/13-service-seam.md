# Contract 13 · The service seam

Status: v1. Decision: ADR 0114. Systems: macOS (launchd), Linux (systemd user unit), Windows
(Task Scheduler). Code: `engine/soyeht-service`, used through `soyeht service`.

An installed Soyeht starts its engine and its process supervisor (`soyeht-ptyd`) when the
person logs in, keeps them running, and lets the installer, the uninstaller and the app register
or remove that start. This contract says what is registered, by whom, what the app does when it
finds no engine, how a failure is restarted, where the logs go, what an uninstall leaves, and
how two profiles live on one machine.

## 1. What is registered

Two services per profile, both for the logged-in user (never the whole machine):

| Component | Program | Arguments |
| --- | --- | --- |
| `ptyd` | `soyeht-ptyd` | `--data-dir DIR` |
| `engine` | `soyeht-engine` | `--profile NAME --data-dir DIR` |

`NAME` is `dev` or `prod`; `DIR` is the profile's data directory (ADR 0112). The profile is an
argument, not an environment variable, because a Windows scheduled task cannot set one and a
service must not depend on the environment it happens to be started from. `--data-dir` is always
written out, so a definition says where its data is without a program having to compute it.

The programs sit in one folder, the installer's choice, named by `--programs` (default: the
folder of the `soyeht` program that runs the command). A definition holds absolute paths. The
folder may be anywhere an installer puts programs (`/usr/bin`, `/usr/local/bin`, `/opt/soyeht`,
`~/.local/bin`, `C:\Program Files\Soyeht`) and needs no "next". It is refused only when it is a
place of the legacy app: inside one of its programs or one of its data folders, matched as whole
folder names without regard to case (so a `.next` folder beside a legacy name is fine). The error
names the place it matched. The list is `contracts/legacy.json`, generated for Rust and Python;
no guard keeps a copy. The
data directory is the other way round: it must carry "next".

**Names come from `contracts/profiles.json`** and nowhere else:

| System | What | Dev | Prod |
| --- | --- | --- | --- |
| macOS | launchd label, and `<label>.plist` in `~/Library/LaunchAgents` | `launchdLabels` | `launchdLabels` |
| Linux | user unit file in `$XDG_CONFIG_HOME/systemd/user` (else `~/.config/systemd/user`) | `systemdUnits` | `systemdUnits` |
| Windows | task name in Task Scheduler | `windowsTasks` | `windowsTasks` |

Every name carries "next" (ADR 0112); the generator refuses one that does not, and so does the
renderer. A name or a place of the legacy app is refused before anything is written
(`soyeht_service::guard`).

## 2. What each system is told

| | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Start | `RunAtLoad` | `WantedBy=default.target`, enabled | `LogonTrigger` for this user (its SID) |
| Restart | `KeepAlive` (however it ended, while the program exists: `PathState`), `ThrottleInterval` 1 s | `Restart=always`, `RestartSec=1`, `StartLimitIntervalSec=0` | `RestartOnFailure`, every minute, up to 999 times |
| Program gone | `KeepAlive` `PathState` on the program: launchd stops restarting it (see section 5) | `ConditionPathExists=` the program: the unit is skipped, not failed, so no restart loop (ADR 0121) | none |
| Order | none: the engine reconnects | engine `Wants=` and `After=` the supervisor (never `Requires=`) | none: the engine reconnects |
| Stand-in for a login shell | `PATH` with the system and user tool folders, `HOME`, `USER` | `PATH` as on the Mac | the user's own environment |
| Log | `StandardOutPath`/`StandardErrorPath` = `logs/<component>.log` | `StandardOutput=append:` the same file | the host appends the program's output to the same file (section 6) |
| Limits | `ProcessType` Interactive | none | no time limit; one instance; runs on battery; normal priority |
| Account | the user's launchd domain: `gui/<uid>` if it exists, else `user/<uid>` | the user's manager | interactive token, least privilege |

`StandardOutput=append:` needs systemd 240 or later (Ubuntu 20.04 and later, Debian 10 and later,
Fedora 30 and later). An older systemd ignores the line and the output goes to the journal
(Ubuntu 18.04 and RHEL 8 are in that case).

The text of each file is rendered by one pure function, `soyeht_service::render(system,
&Definition)`, tested on every system. A Windows task file is UTF-16 with a byte order mark, the
encoding Windows writes when it exports a task.

The engine does not require the supervisor: restarting the supervisor must not restart the
engine, and stopping the engine must not end the supervisor, since the supervisor holds the
terminals (ADR 0108).

## 3. Who registers

Today the seam is called by the `soyeht service` command, by `scripts/install_dev.py` and by the
Mac app (ADR 0120) and the Windows installer (ADR 0123). The Linux packages leave the registration to the
person (ADR 0121); the Linux and Windows desktop apps do not call it yet. The rest of this section is what they are to do.

**The installer** is to register, with the person's consent to install: it copies the programs and
runs `soyeht service install --programs DIR`. The Windows installer asks with a box, unchecked by
default ("Start Soyeht when I log in"), and registers nothing when it stays unchecked. **The uninstaller** is to run `soyeht uninstall`
(`soyeht uninstall --delete-data --yes` when the person chose to delete their data) before it
removes the programs. **The app** registers only when the person asks, for a copy that was not
installed by an installer (an unpacked archive, a package manager that runs no script, a dragged
app). It never registers silently, since macOS tells the person about every new background item
and Windows lists the task. The Mac app asks once at first launch and keeps a "Start Soyeht"
button in its setup list; it runs the `soyeht` of its own bundle with `--programs` set to the
folder of its own programs. All of them run the same command, which is idempotent: a registration
that is already in place is rewritten; a program that runs is left running with the definition it
started with, and the new text applies when it is next loaded (`service stop` then `service start`,
or the next login). A launch agent that is loaded but idle is loaded again at once.

**An app that moves.** A definition names the programs by path, so a caller that can be moved
asks `service status --programs DIR` (each service then carries `"current"`: its definition on disk
is, byte for byte, what `install --programs DIR` would write) and runs `service install` only for
a service that is registered and not current. The Mac app does so at every launch. This repairs
the registration of an app dragged from Downloads to Applications, and leaves a right registration
alone: a person who ran `service stop` does not find the services started again by the next
launch, and the system announces no new background item. `install` itself writes a definition file
only when its bytes differ from what is there. The Mac app does not register from a disk image or
a translocated path, which are gone when the image is ejected or the app is moved. A program that
runs when its folder moves keeps running; if it ends before the repair, the system cannot restart
it from the old path.

A caller also compares how many services are registered with how many it expects: one missing
(a deleted file, a half-finished install) is stale like a changed path. A service that is
registered and current but not running is not started by the caller at launch; it offers the
person a start (`service install`, which loads an idle job and leaves a running one alone).

The commands, all printing the state of the services after they ran as JSON
(`{"services": [{"component", "name", "registered", "running"}]}`):

| Command | Effect |
| --- | --- |
| `soyeht [--profile P] service install [--component C] [--programs DIR]` | Writes the definitions, registers and starts them (supervisor first). A component whose program already runs, which the window starts when nothing is registered, is registered and not started: the program holds a singleton lock in the data directory (`engine.lock`, `ptyd.lock`), and a second copy would fail on it and be restarted for ever, so the logon trigger starts it next time. Ask the lock, never the service manager. |
| `soyeht service uninstall [--component C]` | Stops (engine first), unregisters, deletes the definitions. Data and logs stay. |
| `soyeht uninstall [--delete-data [--yes]]` | `service uninstall` of both components, and with `--delete-data` the profile's data directory too (section 7). |
| `soyeht service start [--component C]` | Starts what is registered and not running. Fails with "not registered" otherwise, and on Linux and the Mac with "the program ... does not exist" when the program is gone (systemd would skip the unit, and launchd would not run the agent, and both would report success). |
| `soyeht service stop [--component C]` | Stops what runs (engine first). Stays registered. |
| `soyeht service status [--component C] [--programs DIR]` | Reports. Changes nothing. With `--programs`, each service also says whether its definition is current for that folder. |
| `soyeht service render --system S --name N [...] -- PROGRAM ARGS` | Prints the file for explicit values without touching the system. For scripts that start disposable services. |

`registered` means the system will start it at login: the definition is in place (macOS), the
unit is enabled (Linux), the task exists (Windows). `running` means it runs now.

## 4. How the app finds an engine

Planned for the Linux and Windows app: it does only what step 2 describes (ADR 0102) and no app
there asks the seam to start anything yet. The Mac app has no start of its own (section 9): it
runs `soyeht service install` when the person says so (section 3), and connects with its backoff
once the engine listens. The steps below are the behaviour to build for the others.

The endpoint is the truth: the socket of the data directory (Windows: the endpoint file, ADR
0108). The app never asks a service manager whether an engine runs. When a connection fails
because nothing listens:

1. If the profile's service is **registered**, the app is to ask for it to be started (`service
   start`) and wait for the endpoint as ADR 0102 does. It starts no program of its own.
2. If it is **not registered**, the Linux and Windows app falls back to ADR 0102 (this is the
   behaviour of today): it starts the engine and the supervisor detached from the folder of its
   own executable, and may offer to register them. The Mac app starts nothing and says the engine
   is not running.
3. If the start in step 1 fails, the app is to show the failure and the log path. It must not fall
   back to step 2: an engine the app started beside a registered service would make the service
   fail on the engine's lock and be restarted for ever.

An explicit endpoint (`SOYEHT_ENGINE_SOCKET`) is left alone, as before.

## 5. Restart policy

The system restarts each program after any end, one second later (a minute on Windows, the
shortest Task Scheduler allows). The engine stopped alone keeps every terminal: the supervisor
runs on, and the new engine adopts the processes. The supervisor lost alone costs the terminals
the time to restart; the engine reconnects as it already does. On Windows a task that ended
with exit code 0 is not restarted until the next logon, and neither is a task whose 999
restarts are spent.

On the Mac, launchd can still be removing a job for a moment after `bootout`, and then refuses a
new `bootstrap` with error 5 ("Input/output error") or 37 ("Operation already in progress"). The
seam tries again up to four more times, 0.2 s, 0.4 s, 0.6 s and 0.8 s apart (about two seconds),
and only for those two errors; any other failure is returned at once.

**A program that is gone** (the app was dragged to the Trash, or the programs were deleted) does not
leave a failing restart loop on the Mac: the agent's `KeepAlive` is `PathState` on the program, so
launchd keeps it alive only while the file exists. At the next login `RunAtLoad` tries once and
the start fails once; after that nothing runs and nothing is retried. When the program comes back
(an app dragged back, or installed again) launchd starts it by itself. `soyeht service start` of
an agent whose program is gone fails with "the program ... is missing", as on Linux, instead of
loading an agent that cannot run. `soyeht uninstall` is still the way to remove the registration.
On the Mac, a program that is already running when its file is deleted ends itself (ADR 0130): the engine and the
supervisor look at the path they were started from every 10 seconds, and when it is missing three
looks in a row they exit with code 0, so the agent stays loaded, idle and inert.
An app update that replaces the bundle briefly removes the program. Whether launchd's `PathState`
watch re-arms and starts the program again when the bundle is replaced by a rename (how an update
or a drag over the old copy does it) is **untested**: it was not run, and this contract does not
promise it. The recovery that does not depend on it is the app's "Start Soyeht" offer, shown
whenever the services are registered and right but the engine does not answer, whose action is
`service install`.

*Manual check (not run: it needs a real login session and registers services, which was not done
on the machine where this was written).* Install the production app, say yes to the background
start, then `launchctl print gui/$(id -u)/com.soyeht.next.engine` shows `state = running` and a
`keepalive` with the program path. Move the app out of Applications: within about a minute the engine
and the supervisor exit by themselves (ADR 0130) and do not come back, and `soyeht service start --component engine` from the moved app's folder reports the
program missing for the old path. Move the app back: the engine starts without any command.

A program started by hand while its service is registered fails on its lock and the system
restarts the service, which fails again each second. Stop the service first (`soyeht service
stop`).

## 6. Logs

The two programs append to `logs/engine.log` and `logs/ptyd.log` in the profile's data
directory, the files the app's own start writes (`process::detached::spawn_service`). The folder
and the files are owner-only. The engine bounds its own file (16 MiB, four rotated files).
`soyeht service install` creates them, since neither launchd nor systemd makes a folder.

A Task Scheduler task cannot redirect output or hide the console of a console program. The task
therefore starts `soyeht-service-host`, a Windows-subsystem program (no window of its own), as

```text
soyeht-service-host --log FILE -- PROGRAM [ARGUMENT]...
```

The host starts `PROGRAM` with a hidden console and its output appended to `FILE` (the same call
the app's own start uses, without leaving the task's job), waits for it, and exits with its exit
code. The host passes the program's exit code on whole; its own are 2 (a command line it does not
understand), 3 (a log it cannot write to) and 1 (a program it could not start, with the reason in
the log). The task so runs as long as the program does, a failure reaches `RestartOnFailure`, and
ending the task ends the program. The host must sit in the folder of `--programs` on Windows;
`install` refuses a missing host like a missing program. A host that cannot start the program
ends with 1 and writes the reason into the log.

## 7. Uninstall

`soyeht uninstall` is the one command a person or an uninstaller runs. It stops the engine and
the supervisor of the profile, removes their registration and the definition files the seam wrote
(`service uninstall` of both components), and prints the state of the services and
`"dataDeleted"`. It works on all three systems and never connects to the engine. Stopping the
supervisor ends the person's terminals.

**It also ends a program that no registration covers**, the copy the Linux and Windows window
started (section 4, step 2). Before anything is ended it leaves a file `uninstalled` in the data
directory. After a component is unregistered, the engine first, a program that still holds its
singleton lock is asked to end: it wrote its process id into the lock file when it took the lock,
and the id is read only while the lock is held, twice, so that an id left by a program that ended
is never used (ADR 0131). Unix sends the stop request: the engine stops cleanly, the supervisor
(no handler) ends at once with its terminals. Windows ends the process, and its children with it.
The command waits up to ten seconds for the lock to be free before it asks the supervisor, and
tries every component, to unregister it and to end its program, whatever happens to the one before. A program that does not end, that cannot
be asked, or that holds the lock with no id (one of an older build) makes the command fail once
both were tried; the registrations that could be removed are removed by then.

**An open window does not start them again.** Before anything is ended, the command writes a file
`uninstalled` in the data directory, holding a value that no other uninstall writes. `service
uninstall` writes it too, for one component as well (`--component engine`): an open window must not
start the engine again. A window reads it when it begins and the window's start of the engine (section
4, step 2) does nothing when it is new or changed since, saying that Soyeht was uninstalled; a file
that was already there is the baseline, so a window opened after an uninstall starts afresh. The
start does nothing either when the engine's lock file it had an engine with is gone (`--delete-data`),
unless the folder cannot be looked into. Only `service
install` removes the file; a window never does.

**Data stays by default.** The programs, the data directory, the database and the logs are left
alone. `soyeht uninstall --delete-data` also deletes the profile's data directory (workspaces,
transcripts, settings, logs), and only after the person types the profile's name at the prompt;
`--yes` confirms for a script (`--yes` alone is refused).

**The order is: check, confirm, unregister, wait, delete.** The check (`plan_removal`) changes
nothing and runs first, before the prompt and before any service is touched, so a deletion that
would be refused leaves the services registered. After the services are unregistered the command
waits up to 15 seconds for the engine and the supervisor to stop answering on their endpoints; if
one still does (it does not end when asked, above), it deletes nothing and says so, and the command
can be run again.

**Only the profile's own folder is deleted.** Every check reads canonical paths
(`soyeht_platform::fs::canonicalize`), and the directory must be the folder `contracts/profiles.json`
names for the profile below the system's folder for application data. So `--data-dir` pointing at
anything else, however its name reads (`nextcloud`, `~/Documents/nextjs-app`), is refused, and so is:

- the legacy app's folders, however they are reached (a link or an alternate spelling resolves to
  them first), and any path that is relative or has a `.` or `..` component;
- a data directory that is itself a link;
- a folder that holds the home folder, however the home is spelled.

A link inside the directory is removed, never followed (on Windows a link to a folder is removed
as a folder). The data directory of another profile that lies inside this one (Dev is inside
prod's) is kept, so deleting prod's data leaves Dev's.
Programs are the installer's to remove, since it chose where they are. The command touches only
the names and the directory of its own profile (`--profile P`), and never a name or a place that
lacks "next".

## 8. Two profiles on one machine

Dev and prod have different names on every system, different data directories (Dev is inside
prod's), different ports and different sockets, so their services are independent and the
renderer refuses a name that would collide (a test checks the names of both profiles on all
three systems). `soyeht --profile P service ...` touches the profile `P` only. A Dev build
installed over a prod install replaces nothing of it.

## 9. Not covered

The desktop app (Linux and Windows) does not call the seam yet, and of the release installers only the Windows one
does (section 3); the Mac app does through the section 3 command. Signing and notarization of the Mac package are ADR
0120. Auto-update, a machine-wide (all users) service, and starting at boot rather than at login are out of scope.
