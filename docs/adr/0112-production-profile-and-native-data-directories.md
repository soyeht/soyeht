# ADR 0112: A production profile, and native data directories on every system

- Status: accepted
- Date: 2026-10-27

## Context

Until now the repository knew one installed profile, Dev, and every program found it the same
way: `Library/Application Support/Soyeht.next/dev` under the user's home, on macOS, Linux and
Windows alike (ADR 0084). That is right for development and wrong for a product: Windows keeps
application data in `%LOCALAPPDATA%`, Linux in `$XDG_DATA_HOME`, and a production install must
sit beside a Dev install on one machine without sharing a port, a socket or a database.

The owner's Mac also runs an older, unrelated app that is not this repository: bundle
identifiers `com.soyeht.mac` (and `com.soyeht.mac.dev` for its development build), the program
`/Applications/Soyeht.app`, and data in `~/Library/Application Support/Soyeht` (and `Soyeht Dev`).
The rewrite must never read, write, migrate or overwrite any of them, now or at launch.

## Decision

**Every machine identifier of the rewrite carries "next", for ever.** A value without it could
one day equal a legacy one. The product name the person sees is still "Soyeht".

**`contracts/profiles.json` has a `dev` and a `prod` profile.** Each profile has:

- `dataDirectory`: the folder below the system's data base, per system (`macos`, `windows`,
  `linux`), written with `/` between components. Dev is inside the production folder:

  | | base | prod | dev |
  | --- | --- | --- | --- |
  | macOS | `~/Library/Application Support` | `Soyeht.next` | `Soyeht.next/dev` |
  | Windows | `%LOCALAPPDATA%` | `Soyeht.next` | `Soyeht.next\dev` |
  | Linux | `$XDG_DATA_HOME`, else `~/.local/share` | `soyeht-next` | `soyeht-next/dev` |

- `tcpPort`: `8290` for production, `8291` for Dev (nothing else in the repository uses 8290),
  so both run on one machine.
- `appIds`: the Mac app's bundle identifier (`com.soyeht.next.mac`, Dev
  `com.soyeht.next.dev.mac`), which the engine checks when it decides whether a local peer is the
  signed UI, and the Linux application id (`com.soyeht.next`, Dev `com.soyeht.next.dev`), which
  the desktop window sets as its `app_id` and which names the `.desktop` file. The iOS app does
  not read the profile, so its identifier (`com.soyeht.next.ios`) is not declared here.
- `launchdLabels`: `com.soyeht.next.engine` and `com.soyeht.next.ptyd` (Dev
  `com.soyeht.next.dev.*`). The engine's signing identifier is its label.

The socket names (`engine.sock`, `ptyd.sock`) are the same for both profiles and live at the top
level of the file: they sit inside the profile's data directory, which already tells the profiles
apart. `selection` names the two variables below. Other service names (a systemd unit, a Windows
scheduled task, the Windows identity `Soyeht.Next`) are decided when the service seam adds the
program that reads them; a field nothing reads is not added.

**The system's base directory is one function.** `soyeht_platform::dirs::data_base()` returns it
(`soyeht-platform` has no Soyeht dependencies, so it does not know the profile).
`soyeht_protocol::data_directory::data_directory(profile, base)` joins the generated subpath for
the system the program was built for. Swift has its own equivalent for the Mac only
(`ActiveProfile.dataDirectory`).

**A program picks its profile in one way.** Before this change nothing picked: Dev was typed into
every program. Now:

1. the environment variable `SOYEHT_PROFILE` (`dev` or `prod`);
2. else the profile the build defaults to: the value of `SOYEHT_BUILD_PROFILE` when the program
   was compiled, so a release build is built with `SOYEHT_BUILD_PROFILE=prod` and needs no setup;
3. else Dev, so a development build and a test never touch production data.

A name that is not a profile stops the program with an error; it never falls back to Dev, which
would mix the data. `soyeht --profile NAME` overrides the variable for the command line.
`--data-dir` still overrides the directory (not the port or the identifiers). The Mac app, which
is a bundle, takes the profile from the variable, else from its own bundle identifier, else Dev,
and ignores a name it does not know.

**The legacy app is guarded, not only avoided.** `scripts/legacy_guard.py` refuses a service
label, signing identifier or directory that lacks "next" or names a legacy location, and
`scripts/install_dev.py` runs it before it writes anything. The generator refuses a profile
value without "next". `scripts/check_legacy_isolation.py`, a gate step that always runs, checks
the profile and fails on any tracked file that names a legacy identifier or path, except the
files that exist to forbid them. `AGENTS.md` states the rule for agents.

## Consequences

- Windows and Linux Dev data moves from `Library/Application Support/Soyeht.next/dev` under the
  home to the native directory above. The old directories on the Windows and Linux test machines
  are throwaway, so there is no migration; a developer there starts with an empty Dev profile.
  macOS Dev data does not move.
- A production engine and a Dev engine can run side by side: their data, sockets and ports differ.
- The Dev launchd labels, the Dev installer and the Dev launch agent templates are unchanged.
- Out of scope, and built on this: installing the engine as a launch agent, systemd unit or
  scheduled task for production, the engine inside the Mac app, signing, notarization, installers
  and updates. Nothing here reads, migrates or removes data of the legacy app.
