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
| Restart | `KeepAlive` (however it ended), `ThrottleInterval` 1 s | `Restart=always`, `RestartSec=1`, `StartLimitIntervalSec=0` | `RestartOnFailure`, every minute, up to 999 times |
| Order | none: the engine reconnects | engine `Wants=` and `After=` the supervisor (never `Requires=`) | none: the engine reconnects |
| Stand-in for a login shell | `PATH` with the system and user tool folders, `HOME`, `USER` | `PATH` as on the Mac | the user's own environment |
| Log | `StandardOutPath`/`StandardErrorPath` = `logs/<component>.log` | `StandardOutput=append:` the same file | none yet (section 6) |
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

**The installer** registers, with the person's consent to install: it copies the programs and
runs `soyeht service install --programs DIR`. **The uninstaller** runs `soyeht service uninstall`
before it removes the programs. **The app** registers only when the person asks: a first-run
question or a setting ("Start Soyeht when I log in"), for a copy that was not installed by an
installer (an unpacked archive, a package manager that runs no script). It never registers
silently, since macOS tells the person about every new background item and Windows lists the
task. All three run the same command, which is idempotent: a registration that is already in
place is rewritten; a program that runs is left running with the definition it started with, and
the new text applies when it is next loaded (`service stop` then `service start`, or the next
login). A launchd agent that is loaded but idle is loaded again at once.

The commands, all printing the state of the services after they ran as JSON
(`{"services": [{"component", "name", "registered", "running"}]}`):

| Command | Effect |
| --- | --- |
| `soyeht [--profile P] service install [--component C] [--programs DIR]` | Writes the definitions, registers and starts them (supervisor first). |
| `soyeht service uninstall [--component C]` | Stops (engine first), unregisters, deletes the definitions. Data and logs stay. |
| `soyeht service start [--component C]` | Starts what is registered and not running. Fails with "not registered" otherwise. |
| `soyeht service stop [--component C]` | Stops what runs (engine first). Stays registered. |
| `soyeht service status [--component C]` | Reports. Changes nothing. |
| `soyeht service render --system S --name N [...] -- PROGRAM ARGS` | Prints the file for explicit values without touching the system. For scripts that start disposable services. |

`registered` means the system will start it at login: the definition is in place (macOS), the
unit is enabled (Linux), the task exists (Windows). `running` means it runs now.

## 4. How the app finds an engine

The endpoint is the truth: the socket of the data directory (Windows: the endpoint file, ADR
0108). The app never asks a service manager whether an engine runs. When a connection fails
because nothing listens:

1. If the profile's service is **registered**, the app asks for it to be started (`service
   start`) and waits for the endpoint as ADR 0102 does. It starts no program of its own.
2. If it is **not registered**, the Linux and Windows app falls back to ADR 0102: it starts the
   engine and the supervisor detached from the folder of its own executable, and may offer to
   register them. The Mac app starts nothing and says the engine is not running.
3. If the start in step 1 fails, the app shows the failure and the log path. It does not fall
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

A program started by hand while its service is registered fails on its lock and the system
restarts the service, which fails again each second. Stop the service first (`soyeht service
stop`).

## 6. Logs

The two programs append to `logs/engine.log` and `logs/ptyd.log` in the profile's data
directory, the files the app's own start writes (`process::detached::spawn_service`). The folder
and the files are owner-only. The engine bounds its own file (16 MiB, four rotated files).
`soyeht service install` creates them, since neither launchd nor systemd makes a folder.

A Task Scheduler task cannot redirect output, so on Windows a service started by the scheduler
writes to the console the scheduler opens for it and keeps no file. This is a known gap with a
named follow-up (ADR 0114, open questions).

## 7. Uninstall

`service uninstall` stops the programs and removes the registration and the definition files it
wrote. Stopping the supervisor ends the person's terminals. It leaves the programs, the data
directory, the database and the logs: deleting a person's data is a separate choice for the
uninstaller to ask. It touches only the names of its own profile, and never a name or a place
that lacks "next".

## 8. Two profiles on one machine

Dev and prod have different names on every system, different data directories (Dev is inside
prod's), different ports and different sockets, so their services are independent and the
renderer refuses a name that would collide (a test checks the names of both profiles on all
three systems). `soyeht --profile P service ...` touches the profile `P` only. A Dev build
installed over a prod install replaces nothing of it.

## 9. Not covered

The Mac app, the desktop app and the installers do not call the seam yet; the contract above is
what they call. Signing and notarization, auto-update, a machine-wide (all users) service, and
starting at boot rather than at login are out of scope.
