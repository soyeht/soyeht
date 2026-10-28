# ADR 0114: The service seam starts the engine and the supervisor at login

- Status: accepted
- Date: 2026-10-28

## Context

An installed Soyeht must have its engine and its supervisor running without a person starting
them. Before this change, three separate things existed, none of them a product:

- **The Mac Dev installer** (`scripts/install_dev.py`) signs the programs, copies them into the
  profile's `bin/`, and installs two launchd user agents. It read a generated template
  (`engine/soyeht-*/launchd/dev.plist`) for the label, `KeepAlive`, `RunAtLoad`,
  `ThrottleInterval` and `ProcessType`, then overwrote the arguments, the environment and the
  log paths with its own Python code, wrote the file with `plistlib`, and ran `launchctl
  bootstrap`. `scripts/e2e_platform.py` read the same templates for its disposable agents.
  Two writers of a launchd file, and only for Dev.
- **The desktop window** (ADR 0102, `apps/desktop/.../autostart.rs`) starts a detached engine
  and supervisor when it finds none, from the folder of its own executable. ADR 0102 names this
  as the interim that the service seam replaces. PR #632 (ADR 0108) made that start work again
  after a crash on Windows: a stale endpoint file is told from a live listener by a lock, and
  the engine starts a lost supervisor again when it was given `--restart-supervisor`. ADR 0108
  leaves "whoever keeps the supervisor alive" to the launch agent on the Mac and to the service
  seam on the other systems.
- **The Mac app** starts nothing. It connects to the profile's socket and retries with a
  backoff, and the readiness checklist says "The Soyeht engine is not running". It relies on
  the launch agents.

`contracts/profiles.json` has the launchd labels but no systemd unit or Windows task names; ADR
0112 deferred those to "when the service seam adds the program that reads them".

## Decision

**One crate, one command, three renderers.** `engine/soyeht-service` is a new Layer 3 crate
(protocol, platform and process only; the command line is its only user). It has the same
public shape on every system and no `cfg` except `System::current()`:

- `Layout` holds the facts about the machine and builds the `Definition` of each component
  from the profile.
- `render(system, &Definition)` turns a definition into the file of that system: a launchd
  property list, a systemd user unit, a Task Scheduler task (XML, written as UTF-16). It is
  pure, and the three renderings are unit-tested on every host.
- `Manager` writes the file (`soyeht_platform::fs::write_atomic`, owner-only) and runs the
  system's tool through `soyeht_process::run` behind the `Commands` trait: `launchctl`,
  `systemctl --user`, `schtasks` and, for the state of a task, `powershell.exe`. Tests use a
  recording fake and a temporary folder; nothing in the tests reaches a real service manager.

`soyeht service install|uninstall|start|stop|status` is the command line over the manager, and
`soyeht service render` prints a file for explicit values. `docs/contracts/13-service-seam.md`
is the contract: what is registered, by whom, restart, logs, uninstall, profiles side by side.

**Names are data.** `profiles.json` gets `systemdUnits` and `windowsTasks` beside
`launchdLabels`; the generator validates them (characters safe for `systemctl` and `schtasks`
without quoting, distinct, for their component, "next" in each) and writes them into
`soyeht_protocol::profile`. Nothing types a label, unit or task name. The old
`launchd/dev.plist` templates are deleted: the renderer is the only place that knows what a
service file says.

**`install_dev.py` and the e2e launcher use the renderer.** `install_dev.py` still signs and
copies; it then runs `soyeht --profile dev service install --programs DATA/bin` for each
component, and asks `service status` before it replaces a binary. `e2e_platform.py` renders its
disposable agents with `soyeht service render`. Neither writes a plist, calls `launchctl` or
keeps a template. The installer's `--domain` option is gone: the seam uses the graphical
domain when the user has one and the background domain otherwise (what the e2e launcher always
did). The refusal to replace a service now looks at whether it runs, not whether it is loaded,
since what it protects is the person's terminals.

**The engine takes `--profile NAME`.** A Windows task cannot set `SOYEHT_PROFILE`, and a
service should not depend on an inherited environment, so the engine accepts the profile as an
argument like the command line already does (the environment, then the build, still apply when
it is absent). The supervisor needs no profile: it is given its data directory.

**Who registers: the installer always, the app when asked.** See the contract, section 3. The
app never registers silently.

**How the app finds an engine: the endpoint, and the service before its own spawn.** When a
connection finds nothing listening, a registered service is asked to start; ADR 0102's detached
start is for an unregistered copy only, and is not a fallback for a registered service that
failed to start (an engine the app started beside a registered service would make the service
fail on the engine's lock and be restarted every second). The Mac app starts nothing.

**Restart policy** is `KeepAlive` and one second on launchd, `Restart=always` and one second on
systemd, `RestartOnFailure` every minute on Windows. The engine does not `Require` the
supervisor on systemd, and no unit stops the supervisor when the engine stops: the supervisor
holds the terminals. The engine on Windows is not given `--restart-supervisor`: the task already
restarts the supervisor, and two restarters would race on the supervisor's lock and make the
loser's task fail and restart for 999 minutes.

**Logs**: `logs/engine.log` and `logs/ptyd.log` in the data directory on the Mac and Linux, the
same files the app's own start writes.

**A path in a definition is absolute and plain, and the program must exist.** The guard refuses a
relative path and any `.` or `..` component (a `..` could lead out of a folder named "next" into a
legacy one), and `install` refuses a program that is not there, since the system would restart a
failing start for ever.

**Uninstall** removes the registration and the files the seam wrote, and nothing of the person's
data. **Two profiles** have disjoint names, directories, ports and sockets; a test checks that
no two names of the two profiles collide on any system, and the renderer and the guard refuse a
name without "next" or one that belongs to the legacy app (ADR 0112).

**The guard has two rules.** Names (label, unit, task) and the data directory must carry "next"
(ADR 0112). The folder the programs are installed in is the installer's choice and needs no
marker (`/usr/bin`, `/opt/soyeht`, `C:\Program Files\Soyeht`); it is refused only when it is
inside a legacy program or data folder, and the error names the place it matched. The legacy
list itself moves to `contracts/legacy.json`, generated into `soyeht_protocol::legacy` and
`scripts/generated_legacy.py`, so `legacy_guard.py` and the Rust guard share one list.

## Consequences

- The Mac Dev installer gets shorter and has no second copy of a launchd file. Its
  agents change in one respect: the engine's arguments now include `--profile dev`. An agent
  that is already loaded and running keeps its old definition, and still works, until it is
  stopped and started again or the person logs in again; one that is loaded and idle is loaded
  again by the next install.
- A production install is `soyeht service install` away on all three systems once the programs
  are in place. The installers, the Mac app and the desktop app do not call it in this change;
  the contract is what they call.
- The Windows service runs in the console that Task Scheduler opens and keeps no log file. This
  is the one place where Windows is behind the other systems.
- Linux units start at login. They start at boot only if the person enables lingering
  (`loginctl enable-linger`), which the seam does not do.
- A program started by hand while its service is registered makes the service restart in a loop
  until it is stopped. The contract says to stop the service first.

## Not verified

The three renderings are tested as text and, on this Mac, the property list was linted with
`plutil` and the task file parsed as UTF-16 XML. No `systemd`, `launchd` or Task Scheduler
accepted any of the files in this change: the command sequences were checked against a fake
runner, never against a real service manager. In particular the Task Scheduler XML, the
`schtasks /Create /XML` call and the PowerShell state query need a run on Windows, and the unit
with `StandardOutput=append:` needs systemd 240 or later.

## Open questions for the owner

1. **Mac registration style.** The seam writes a plist into `~/Library/LaunchAgents`, like the
   Dev installer. A notarized app may prefer `SMAppService`, whose plist lives inside the bundle
   and which shows the item in System Settings with the app's name. The renderer is one
   function, so a second Mac renderer is local; the choice belongs with signing and packaging.
2. **A hidden window and a log on Windows.** A console program started by Task Scheduler opens
   a console window and cannot redirect output. The fix is a Windows-subsystem host that starts
   the program with a hidden console and appends to `logs/*.log` (the way `spawn_service`
   does), or `conhost.exe --headless`, which has not been tried. Until then the person sees two
   console windows at logon.
3. **Starting at boot.** Login only, by default. Enabling lingering and a machine-wide task are
   product choices.
4. **First-run registration by the app.** The default is to ask. Registering silently on first
   run would make the app work with no installer at the cost of a macOS notification the person
   did not ask for.
5. **Where the programs live on the Mac.** The Dev profile keeps signed copies in
   `<data>/bin` so that privacy grants stay with a stable path. A production app may point the
   agents into the bundle instead. `--programs` takes either.
