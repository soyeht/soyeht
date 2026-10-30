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
module `scripts/packaging_common.py`, which `package_linux.py` now uses too.

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
  and cannot be a place of the legacy app). Installer id and registry key `Soyeht.Next` (`Soyeht.Next_is1`),
  both derived by the script from the production profile's task names (`Soyeht.Next.Engine`,
  `Soyeht.Next.Ptyd`) so they cannot drift from them; one Start menu shortcut, "Soyeht".
- An x64 installer also opens on Windows on Arm (`x64compatible`); an Arm64 one opens only on Arm.
- **No silent registration.** The wizard has a task "Start Soyeht when I log in", unchecked. Ticking it runs
  `soyeht service install --programs <folder>`; silent installs opt in with `/TASKS=loginstart`. Otherwise
  the first run works as ADR 0102 describes. The wizard remembers the choice for the next upgrade.

### Upgrade

The newer installer runs over the older one. The install folder, the tasks and the data are unchanged.

- The installer asks Windows' Restart Manager to close the window (the filter names only
  `soyeht-desktop.exe`). Closing the window ends nothing else. It is best effort: when the window stays open
  (it did for the x64 build on the Arm64 VM, and closed for the Arm64 build) it keeps running from its
  renamed file like the others.
- The engine and the supervisor are not stopped. Windows allows renaming, not overwriting, a running
  program, so before the new files are written each installed program is renamed to
  `NAME.exe.<n>.old`; the new ones take its place, and the `.old` files that can be deleted are. A
  running engine or supervisor continues from its renamed file, with the terminals it holds, and the
  installer tells the person so (a window left open too). The new build starts the next time that program starts: the registered
  tasks restart on failure and at login, and `soyeht service stop` then `start` applies it at once (a
  supervisor restart ends the terminals, which is the person's decision, not the installer's). This is the
  Windows form of the AppImage rule: replacing the programs never ends a running supervisor.
- Leftover `.old` files of a program that still runs are deleted by the next install or by the uninstall.

### Uninstall

After Inno's own "are you sure", the uninstaller asks one question, "Also delete your data?", with No as the
default (a silent removal keeps the data unless `/DELETEDATA` is given). Then, in order: `soyeht uninstall` (stops and unregisters the profile's
tasks, keeps the data; ADR 0118); the engine and supervisor the window started, which are in no task and would
lock their files, ended by process identifier and only if their executable is in the install folder; with
"delete", `soyeht uninstall --delete-data --yes`; then Inno removes the programs, the shortcut and the
registry key. Only the install folder is removed from the programs' side, and the data directory only
through `soyeht uninstall`, which already refuses anything but the profile's own folder.

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
  `%LOCALAPPDATA%\Programs\Soyeht Next`, the shortcut "Soyeht" in the Start menu, the entry `Soyeht.Next_is1` in
  `HKCU`, and no scheduled task. The shortcut, opened from the Start menu search, opens the window for both
  architectures, which starts `soyeht-engine.exe` and `soyeht-ptyd.exe` from the install folder.
- `/TASKS=loginstart`: the tasks `Soyeht.Next.Engine` and `Soyeht.Next.Ptyd` are registered and running through
  `soyeht-service-host.exe`, with the path (it contains a space) quoted; a window opened afterwards attached to
  that engine and started none.
- Upgrade (0.1.0 to 0.1.1, bumped on the VM only): with a window, the engine and the supervisor running, the
  new installer exited 0 with no restart; the engine and supervisor kept their process identifiers, the files of
  the running programs were renamed `*.old` and replaced, the shortcut and the registry entry showed 0.1.1. The
  same with the registered tasks running (the service host is renamed too, and the tasks kept running). The
  window closed on the arm64 upgrade and stayed open on the x64 one.
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
SmartScreen or antivirus policy; the interactive uninstall after the final change of the dialog; an upgrade whose
new build changes the supervisor's protocol; a registration made while a window-started engine already runs
(the task's program fails on the lock and Task Scheduler retries it every minute until the first stops); an
MSI or MSIX (not built).
