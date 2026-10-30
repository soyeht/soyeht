# ADR 0123: The Windows app ships as a per-user Inno Setup installer

- Status: accepted
- Date: 2026-10-30

## Context

Windows had the programs and the pieces around them (a window with no console, the icon and version
resource, `soyeht service install`, `soyeht uninstall`, the engine started by the window) and nothing a
stranger could run. A release is one folder: `soyeht-desktop.exe`, `soyeht-engine.exe`, `soyeht-ptyd.exe`,
`soyeht-mcp.exe`, `soyeht.exe` and `soyeht-service-host.exe`. The decisions that shape how it is installed
are earlier ones:

- the window starts the engine and the supervisor from the folder of its own executable (ADR 0102), and the
  login tasks hold the absolute path of each program (ADR 0114, contract 13), so the folder must not move
  between versions;
- registering the login tasks needs the person's consent (contract 13, section 3);
- restarting the supervisor ends the person's terminals, restarting the engine does not (contract 07);
- the data directory (`Soyeht.next`) belongs to the person, and removal keeps it unless asked (ADR 0118);
- every machine identifier of the rewrite contains "next" (ADR 0112).

The goal: a per-user installer (no administrator) for x64 and Arm64, for the production profile, that
installs, upgrades in place and uninstalls.

## Decision

**Inno Setup 6, one `.exe` per architecture.** `scripts/package_windows.py` builds both cargo workspaces in
release mode for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` with `SOYEHT_BUILD_PROFILE=prod`,
checks each program's PE header (the processor, and that the window and the service host open no console),
signs when asked to, and runs `ISCC` on `apps/desktop/packaging/windows/soyeht.iss`. The output is
`dist/soyeht-<version>-windows-<x86_64|aarch64>.exe`, the names the update manifest expects. The version is
`contracts/release.json` and nothing else (ADR 0115): the script passes it, with the profile and every name,
to the installer script through `ISCC /D` options, and a test fails if the script reads a value the
command line does not give it. The version, naming, plan, release build and program lookup are the shared
module `scripts/package_common.py`, which `package_linux.py` and `package_mac.py` use too (ADR 0120, ADR 0121).

### Why not an MSI or an MSIX

*MSI (WiX 5 or 6, a .NET tool).* A per-user MSI is possible (`Scope="perUser"`, files under
`LocalAppDataFolder`) and is what an administrator would deploy by policy. It was set aside for this
launch, not ruled out. It needs a second toolchain (the .NET SDK and WiX) on every build machine. What this
installer must do beyond copying files is not what an MSI does well: a consent box that registers tasks only
when ticked, a "keep or delete my data" question at removal, and handling running programs. Each is a
custom dialog plus a custom action sequenced against file removal. Windows Installer replaces a file in use
through the Restart Manager, which asks the owner to close or restarts the machine; moving a running
supervisor aside is a custom action of our own. The same staging folder can feed an MSI later.

*MSIX.* Ruled out by its model, from Microsoft's documentation (a package was not built here). A package
installs below `C:\Program Files\WindowsApps\<name>_<version>_<arch>__<publisher id>`, read-only, in a path
that changes with every version, which is exactly what contract 13 forbids (a task holds an absolute path) and
what ADR 0121 avoids for the AppImage; a program inside it is started through its package, not by a path from
a scheduled task, and the command line would need an execution alias. A packaged desktop app also runs with
file-system and registry virtualisation: writes below `%LOCALAPPDATA%` land in the package's private
`LocalCache` and are deleted with it, so the data directory ADR 0112 names would not be where the engine,
the command line and `soyeht uninstall` expect it unless the package declares an unvirtualised-resources
capability (restricted, reviewed by the Store). The engine and the supervisor are launched as detached
children of the window and must outlive it; both are `runFullTrust` territory with the same identity
inheritance. And a package must be signed with a certificate the machine trusts to be installed at all, which
a stranger without a purchased certificate cannot do; Inno has no such condition: an unsigned installer only
shows a SmartScreen warning.

*Inno Setup* gives a per-user install with `PrivilegesRequired=lowest` (no elevation, no choice of "all users"),
a Pascal script for the three behaviours above, an uninstaller that is part of the same file and registers
in Add or remove programs under `HKCU`, silent switches for the update flow, and a signing hook that signs the
installer and the uninstaller. Inno Setup 6.7 compiles to a 32-bit setup that runs on x86, x64 and Arm64
Windows (emulated on the last); `ArchitecturesInstallIn64BitMode` installs the programs of the package's
own architecture. The licence permits free commercial use.

### What an install is

- Folder `%LOCALAPPDATA%\Programs\Soyeht Next` (no directory page: the folder carries "next", does not move,
  and cannot be a place of the legacy app). All of it comes from `contracts/profiles.json`, never typed in
  the script: the folder is the profile's `displayNames.macApp` (`Soyeht Next`), the installer id is
  `appIds.windows` (`com.soyeht.next`, so the key of Add or remove programs is `com.soyeht.next_is1`), and
  the name of the shortcut, of the entry in Add or remove programs and of the signature's description is
  `displayNames.window` (`Soyeht`). `check_names` passes the id and the folder through `legacy_guard.require_next`.
- An x64 installer also opens on Windows on Arm (`x64compatible`); an Arm64 one opens only on Arm.
- **No silent registration.** The wizard has a task "Start Soyeht when I log in", unchecked. Ticking it runs
  `soyeht service install --programs <folder>`; silent installs opt in with `/TASKS=loginstart`. Otherwise
  the first run works as ADR 0102 describes. The wizard remembers the choice for the next upgrade.

### Only this folder, only these programs

The installer moves, renames and deletes programs, so it refuses any folder but
`%LOCALAPPDATA%\Programs\Soyeht Next`. `DisableDirPage=yes` hides the page but not `/DIR=` or a remembered
folder, so the folder is checked at the start for `/DIR=` (a silent run ends with exit code 1 and no message
box) and again before the first file (`PrepareToInstall`) for the folder Setup resolved. Everything it renames,
deletes or ends is limited to the shipped names: `soyeht*.exe`, and their moved copies `soyeht*.exe.*.old`;
never a bare `*.exe` or `*.old`, and there is no `[InstallDelete]` entry. A process is ended only when its
executable is below the install folder and named `soyeht*`.

### Upgrade

The newer installer runs over the older one. The install folder, the tasks and the data are unchanged.

- The installer asks Windows' Restart Manager to close the window (the filter names only
  `soyeht-desktop.exe`). Closing the window ends nothing else. It is best effort: when the window stays open
  (it did for the x64 build on the Arm64 VM, and closed for the Arm64 build) it keeps running from its
  renamed file like the others.
- Windows allows renaming, not overwriting, a running program, so before the new files are written each
  installed program is renamed to `NAME.exe.<stamp>.old` and the new ones take its place. The moved copies
  are deleted only after every file is installed (`ssPostInstall`); when the install does not complete, they
  are renamed back (`DeinitializeSetup`), so a failed upgrade leaves the old build as it was. A copy that
  still runs stays, and the next install or the uninstall removes it.
- **The supervisor is never ended**, since it holds the terminals (contract 07). When something was running
  the installer compares `soyeht-ptyd --protocol-version` of the moved copy and of the new one:
  - *Same protocol* (the usual case), and no window left open: the old engine is ended. The terminals stay
    in the supervisor (contract 07: an engine restart loses nothing), and the new engine is started by the
    next window (ADR 0102) or by its login task (Task Scheduler restarts it at once). A window-started
    engine is in no task, so ending it is what lets the next window start the new build.
  - *Different protocol* (or the old copy cannot answer): nothing is ended. The running engine and supervisor
    keep the previous build, which still works together, and the person is told (a message box when not
    silent, a log line when silent) that the services must restart for the new build, and that restarting
    the supervisor ends the terminals.
  - *A window is still open:* nothing is ended and the person is told to close and open it again.
- A message box is shown only when a person can read it: `WizardSilent` and `UninstallSilent` runs only log.
  A `/VERYSILENT` run without `/SUPPRESSMSGBOXES` can still meet a message box of Setup's own (a file in use
  during the copy, for example); `/SUPPRESSMSGBOXES` is what a script passes.

### Uninstall

After Inno's own "are you sure", the uninstaller asks one question, "Also delete your data?", with No as the
default (a silent removal keeps the data unless `/DELETEDATA` is given). Then, in order: `soyeht uninstall`
(stops and unregisters the profile's tasks, keeps the data; ADR 0118, skipped when `soyeht.exe` is gone);
the programs of the folder that still run (the window's engine and supervisor are in no task and would lock
their files), ended by process identifier, the engine first because it would start a lost supervisor again
(ADR 0108), then the rest, twice in case something restarted; with "delete", `soyeht uninstall
--delete-data --yes`; then Inno removes the programs, the shortcut and the registry key. The data directory
is deleted only through `soyeht uninstall`, which already refuses anything but the profile's own folder.

### Robustness of the script

The paths the PowerShell steps use are passed in environment variables, never in the script text (a user
name such as O'Brien would break a quoted string). Setup is a 32-bit program, so it runs the native PowerShell
(`{sysnative}`) and asks `Win32_Process` for the executable path (the 32-bit `Get-Process` can leave it blank
for a 64-bit process). Every command of the installed `soyeht.exe` runs with a 120 second limit and is ended
when it passes; every exit code is written to Setup's log (`/LOG=`).

### Registering while the programs run

A person may open the window before ticking "Start Soyeht when I log in" (or install over a copy whose window
started the engine). The window's engine and supervisor are in no task, and a task started beside them would
fail on the program's singleton lock and be restarted every minute. So `soyeht service install` (all three
systems, `soyeht-service`) asks `engine.lock` and `ptyd.lock` in the data directory, the files the programs lock
themselves (`soyeht_platform::fs::singleton_held`), registers a component whose program holds its lock and does
not start it; the logon trigger starts it the next time. `AlreadyRunning` of the programs stays an error:
a second copy started by hand still fails on its lock.

### Signing

`SOYEHT_WINDOWS_SIGN_THUMBPRINT` (a certificate in the builder's store) or
`SOYEHT_WINDOWS_SIGN_AZURE_DLIB` with `SOYEHT_WINDOWS_SIGN_AZURE_METADATA` (Azure Artifact Signing) turn
signing on; `SOYEHT_WINDOWS_SIGN_TIMESTAMP_URL` and `SOYEHT_WINDOWS_SIGNTOOL` adjust it. Every program is
signed and verified, then Inno signs the installer and its uninstaller with the same `signtool` command and
the installer is verified. Without a variable the build is unsigned and says so loudly;
`--require-signature` makes that an error for a release. There is no variable for a password or a token:
the certificate stays in a store or at the signing service, and Azure's own credentials come from its usual
environment. Nothing of this is in the repository.

## Consequences

- Windows Defender's machine-learning detection flagged the unsigned `soyeht.exe` (the command line, which
  registers scheduled tasks) on the test VM after a few runs, as `Trojan:Win32/Bearfoos.A!ml`, and blocked it
  from running (an installer that depends on it reports the failure instead of a success). This is a false
  positive of the kind an unsigned, unknown program meets; a signed build and a submission of the programs to
  Microsoft are the remedy, so a release must be signed before a stranger runs it.
- A person who runs an unsigned installer meets SmartScreen. Until a certificate or an Azure signing profile
  exists, a release is not ready for strangers; the hook is the only thing to switch on.
- An upgrade leaves an old engine or supervisor running until it restarts; for a few versions that is
  harmless, and when a protocol change makes it matter the engine reports the supervisor as unavailable
  (contract 07) and the person restarts the services. The installer does not decide to end terminals.
- A silent uninstall that cannot unregister the tasks still removes the programs (it cannot be aborted
  half-way); the person sees a message and the tasks, which fail to start, can be deleted in Task Scheduler.
- The command line is not on `PATH`; a person runs it by its path (`soyeht.exe` in the install folder).
- Toast notifications (ADR 0103) on Windows are not tied to the shortcut by an application id yet.
- Not done: a machine-wide install, an MSI for managed deployment, a winget manifest, the update check
  (ADR 0122), and the `PATH` entry.

## Verification

On a Windows 11 Arm64 VM (24H2), as the signed-in standard user, with Inno Setup 6.7.3 installed per user by
`winget`; both installers built there with `scripts/package_windows.py` (unsigned):

- Build: the arm64 and x64 programs build with `--locked` in release mode; the PE check passed for all six
  programs of both (arm64 `0xAA64`, x64 `0x8664`, window and service host GUI, the others console).
  The arm64 build needs `clang` on `PATH` (`ring`); without it the script stops before cargo with a message.
- Install (silent, no elevation, `User privileges: None`; the arm64 one also through the wizard, whose task page
  shows "Start Soyeht when I log in" unchecked): the six programs, `LICENSE`, `NOTICE` and the uninstaller in
  `%LOCALAPPDATA%\Programs\Soyeht Next`, the shortcut "Soyeht" in the Start menu, the entry `com.soyeht.next_is1` in
  `HKCU`, and no scheduled task. The shortcut, opened from the Start menu search, opens the window for both
  architectures, which starts `soyeht-engine.exe` and `soyeht-ptyd.exe` from the install folder.
- `/TASKS=loginstart`: the tasks `Soyeht.Next.Engine` and `Soyeht.Next.Ptyd` are registered and running through
  `soyeht-service-host.exe`, with the path (it contains a space) quoted; a window opened afterwards attached to
  that engine and started none.
- Upgrade (0.1.0 to 0.1.1, bumped on the VM only), after the review changes, on arm64, with `/VERYSILENT` and no
  `/SUPPRESSMSGBOXES`: with the window, engine and supervisor running, the installer exited 0 and never waited
  for a message box; the window was closed, the supervisor kept its process identifier, the engine (same
  protocol, `5` and `5`) was ended, and opening the window again started a new engine from the new file while
  the supervisor, which still runs from its moved copy, kept its identifier. With the login tasks registered the
  same upgrade ended the engine and Task Scheduler started the new one at once; the supervisor and its task
  were untouched. With a stand-in supervisor that reports another protocol and runs, nothing was ended and the
  log says the services must restart. The earlier x64 run: the window stayed open and kept running from its
  moved copy.
- Registering while the programs run (`soyeht service install`, on the build that holds the lock check): with the
  window's engine and supervisor running and nothing registered, both tasks were registered, none started, and
  no second program ran. With the login tasks registered, an upgrade ended the old engine and the install step
  started the new one at once (the supervisor kept its process identifier); a fresh install with
  `/TASKS=loginstart` started both. A removal then unregistered both tasks and left nothing running.
- A failed upgrade (a file Setup could not replace, with `/SUPPRESSMSGBOXES`): exit code 5, "Rolling back
  changes", and every program renamed back, so the old build was whole. Without `/SUPPRESSMSGBOXES` the same
  failure waits on Setup's own message box (a limit named above).
- `/DIR=` with a folder that holds another program and a `.old` file, the same folder with a trailing
  backslash, and `C:\Tools`: each exited 1 with no message box, and the other program and its hash were
  unchanged. `/DIR=` with the expected folder installs.
- Uninstall: silent, with a window, engine and supervisor started by the window running (no task): the exit
  left nothing in the install folder, no shortcut, no registry key, no task and no program of the install
  running; only the data folder remains. With the tasks registered, they were removed. `/DELETEDATA` deleted
  the profile's data and kept the nested Dev folder, and left alone the engine, supervisor and window of another
  copy that another person had running from a different folder on the same VM (only processes whose
  executable is in the install folder are ended).
- The first version of the uninstaller asked its question before Inno's own "are you sure", which made two
  prompts in a row; the screenshots of the dialog and the data question are from that version and the final
  question (after Inno's prompt) was exercised only through `/DELETEDATA` and the silent default.

Signing was exercised with a throwaway self-signed certificate (in the VM's user store, deleted afterwards)
named by `SOYEHT_WINDOWS_SIGN_THUMBPRINT`: `signtool` signed a program with a DigiCert timestamp, and Inno signed
the installer with the command the script passes through `/S`, timestamp included (Windows reports the chain as
untrusted, as expected for such a certificate, so `signtool verify /pa` was not run).

Not verified: a build signed with a trusted certificate or through Azure Artifact Signing (no certificate or
account exists yet; the Azure command is tested as text only); installing on
x64 hardware (only the x64 build on Windows on Arm, under emulation); Windows 10; a machine with a
SmartScreen or antivirus policy; the interactive uninstall after the final change of the dialog; a user name with an apostrophe (the path goes through an environment variable, which was not tried with one); an upgrade whose
new build changes the supervisor's protocol; an
MSI or MSIX (not built).
