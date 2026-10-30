# ADR 0120: The Mac release package, and how the engine inside it is started

- Status: accepted
- Date: 2026-10-29

## Context

A stranger who downloads Soyeht for the Mac must be able to open one file, drag one app, and
have a working product. Until now `Soyeht Dev.app` held only the Swift app. The engine, the
supervisor, the command line and the MCP program were built in debug, signed with an Apple
Development identity and copied by `scripts/install_dev.py`, which also registered two launchd
agents through `soyeht service install` (ADR 0114). Nothing built a release, nothing was signed
for distribution, and the app started nothing: with no engine it said "The Soyeht engine is not
running".

This ADR covers the production profile of `contracts/profiles.json` (bundle `com.soyeht.next.mac`,
labels `com.soyeht.next.engine` and `.ptyd`, data directory `Soyeht.next`). The Linux package
(ADR 0121), the update check (ADR 0122) and the Windows installer (ADR 0123) are separate.

## Decision

### One script, one output name

`scripts/package_mac.py` (run with `uv run python`) does everything from a clean checkout:

1. builds `soyeht-engine`, `soyeht-ptyd`, `soyeht` and `soyeht-mcp` with `cargo build --release
   --locked --target aarch64-apple-darwin`, with `SOYEHT_BUILD_PROFILE=prod` so none of them can
   fall back to Dev data (ADR 0112);
2. builds the app with `xcodebuild` in Release, with `SOYEHT_APP_BUNDLE_ID`, `SOYEHT_PRODUCT_NAME`
   and the version given as build settings. These two `SOYEHT_*` settings belong to the app target
   (its `PRODUCT_NAME` and display name are `$(SOYEHT_PRODUCT_NAME)`, Dev by default): a global
   `PRODUCT_NAME` on the command line would rename the resource bundle of every Swift package and
   make two commands produce one file. The project itself stays Dev, since the engine's tests read
   its identifier; the version is `contracts/release.json` (ADR 0115), and the script stops when
   `release.py check` finds any place that disagrees;
3. copies the app, puts the four programs in `Contents/MacOS`, and signs every nested program,
   then the app (below);
4. notarizes and staples the app, makes the disk image, signs, notarizes and staples the image;
5. checks the result without opening it: `codesign --verify --deep --strict`; the team and the
   hardened-runtime flag of **every** program, library and bundle inside the app (found by
   scanning, not by name; a missing signature line fails the check); the identifier of the app and
   of each of the four programs; the entitlements of the app and of everything inside it,
   `lipo -archs`, the bundle's identifier and versions, a read-only `hdiutil attach -nobrowse` of
   the image with the same checks on the app inside, and `spctl --assess` of both.

The output is `dist/soyeht-<version>-macos-aarch64.dmg` and `.dmg.sha256` (`dist/` is ignored).
`soyeht-<version>-<os>-<arch>.<ext>` is the name every platform's package follows. A build signed
ad hoc goes to `dist/ad-hoc/` instead, so it can never be mistaken for the release.

**The identity, the team and the notary credentials come only from the environment**:
`SOYEHT_MAC_SIGNING_IDENTITY` (the name or SHA-1 of a "Developer ID Application" certificate),
`SOYEHT_DEVELOPMENT_TEAM` (its ten-character team, the variable the Dev builds already use) and
`SOYEHT_NOTARY_PROFILE` (a keychain profile made once with `xcrun notarytool store-credentials`,
so no password, key or Apple ID is ever in an environment or a file). The script never searches
the keychain for an identity: choosing a certificate by looking is how a package gets signed by
the wrong one. An identity without its team is an error, since the engine attests the app by
team (below).

**Ad hoc.** Without the identity, or with `--ad-hoc`, the whole pipeline runs with `codesign
--sign -`, hardened runtime and no timestamp, skips notarization and says so in a banner that
cannot be missed. `--release` is the opposite: it refuses to go on unless all three variables
are set, so a release machine with a misspelled variable fails instead of producing an ad-hoc
file. An ad-hoc build is for testing the pipeline: Gatekeeper rejects it on every other Mac, and
the engine does not treat it as the signed UI (see "Attestation").

### Apple silicon only

The package is arm64 only, and the script asserts it with `lipo -archs`. The reasons:

- The dictation engines come from `speech-swift`, which is built on MLX, whose Metal backend
  targets Apple silicon. A universal app would have to drop dictation on Intel or carry two code
  paths to test, on machines the project has none of.
- No Intel Mac is available to test on, and macOS 26 is the last release that runs on one.
- The engine programs would need the `x86_64-apple-darwin` target, a second release build of
  every crate (thin LTO with one codegen unit is the slowest build of the project) and `lipo`.

If the owner wants Intel back, the Rust side is a second `--target` and a `lipo -create`; the
Swift side is `ARCHS="arm64 x86_64"` once the dictation package allows it. I did not build
x86_64 to prove the Swift side fails: the reasons above are from the package's manifest.

### A disk image, not a zip

A `.dmg` is what a person expects ("drag to Applications"), it can be signed, notarized and
**stapled itself** (a zip cannot carry a ticket), and it keeps the app's extended attributes and
links exactly. The app inside is notarized and stapled first, so the ticket also travels with the
app when it is copied out and the Mac is offline. The image is made with `hdiutil create
-srcfolder` and no Finder script: it has the app and a link to `/Applications` and no custom
layout, which would need a GUI session. A zip may still be right for the update check (ADR 0122);
this ADR does not decide that.

### The app is called "Soyeht Next"

The bundle is `Soyeht Next.app`. `Soyeht.app` is the legacy app's program on the owner's Mac, and
a DMG that put a `Soyeht.app` in front of `/Applications` would offer to replace it (ADR 0112,
`contracts/legacy.json`). The window title is "Soyeht". Both names are `displayNames` of each
profile in `contracts/profiles.json` (`macApp`, `window`), generated into the Swift, Rust and
Python forms; no script or view types them. The generator checks they are printable text and that
two profiles do not share them. The product name is not an identifier, so the generator does not
demand "next" of it, but the packaging script passes the production name through `require_next`
before it builds anything (the same check as the bundle identifier and every signing identifier),
so a name without it, or one that is the legacy app's, stops the run. When the legacy app is
retired the name is one line of the profile.

### Where the programs are, and how they are signed

The four programs sit in `Contents/MacOS`, next to the app's own executable. The app finds the
command line with `Bundle.url(forAuxiliaryExecutable:)`, with no path of its own to keep in step
with the script, and `soyeht service install` with no `--programs` already means "the folder of
this program". The engine finds `soyeht-mcp` beside itself (see "Release builds" in
`docs/platforms.md`).

Every program is signed individually, deepest first, then the app, **never with `--deep`** (which
signs by guessing and gives every program the app's flags):

- the options are `--options runtime` (hardened runtime) and, with an identity, `--timestamp`
  (secure timestamp);
- the signing identifier of each program is `binary_identifiers(prod)`
  (`scripts/profile_programs.py`, shared with the Dev installer): `com.soyeht.next.engine`,
  `.ptyd`, `.cli` and `.mcp`. The engine's identifier must be its launchd label, since it checks
  its own signature against it;
- the app is signed with `apps/mac/Soyeht.entitlements`, the programs with none.

The Dev app uses the same file (`CODE_SIGN_ENTITLEMENTS` in both configurations of the project),
so dictation in a development build is not denied under hardened runtime either, and there is one
list. Before this the Dev build relied on `ENABLE_RESOURCE_ACCESS_AUDIO_INPUT`, which Xcode turns
into the same entitlement when it signs; I did not build and sign a Dev app to prove that, which
is why the explicit file is there.

**No path of the build machine in a program.** A release is built with `--remap-path-prefix` for the source
folder, the home folder, `CARGO_HOME`, `RUSTUP_HOME` and a custom `CARGO_TARGET_DIR`, so a panic message or
a debug record names `/soyeht/...` and `/cargo/...`, never the builder's user name. The rules are set once, in
`scripts/package_common.py` (`release_environment`), which the Mac, Linux and Windows packagers all build
through (flags in `CARGO_ENCODED_RUSTFLAGS`, since a user name can hold a blank; flags already set are kept).
The C libraries the engine links (SQLite and tree-sitter, built by the `cc` crate) get the same rules as
`-ffile-prefix-map` in `CFLAGS`; Microsoft's compiler has no such flag (one of its assertions printed the
registry path of the builder into `soyeht-engine.exe`), so a Windows release compiles its C with clang
(`CC_<target>`), which LLVM, already a build requirement for Arm, provides. The Swift part of the Mac app is
not covered.

### Entitlements

Hardened runtime closes doors; an entitlement opens one, and a signed app carries what it
needs and nothing else. The script checks the result: the app has exactly the set below and no
program has any.

| Entitlement | Asked | Why |
| --- | --- | --- |
| `com.apple.security.device.audio-input` | app | Dictation records with `AVAudioEngine` (`MicrophoneCapture`). Under hardened runtime a process without this entitlement is **denied the microphone with no prompt**. `NSMicrophoneUsageDescription` and `NSSpeechRecognitionUsageDescription` are already in `Info.plist`. |
| `com.apple.security.app-sandbox` | no | The app and its helpers run shells, agents and `git` as the person (`ENABLE_APP_SANDBOX = NO`). A sandbox would end the product. |
| `network.client`, `network.server` | no | These are sandbox keys. An unsandboxed process can already connect, and the engine's tailnet listener (port 8290) is the engine's, which is not sandboxed. |
| `automation.apple-events` | no | No code in `apps/` sends an Apple event (searched for `NSAppleScript` and `AppleEvent`). |
| `cs.allow-jit`, `cs.allow-unsigned-executable-memory` | no | `WKWebView` runs web content in system processes. MLX builds its kernels in the system's GPU compiler. The Rust programs generate no code. |
| `cs.disable-library-validation` | no | The app loads no library that is not its own. The Swift packages are linked in; the one library in `Contents/Frameworks` is a Swift back-deployment library (`libswiftCompatibilitySpan.dylib`) that the package script signs again with the app's own identity, so library validation (same team) passes. Adding the key would let any library injected by a local attacker load. |
| The camera, personal-information and file-access entitlements | no | Nothing uses them, and the file-access keys are sandbox keys. Speech recognition has no entitlement on the Mac, only the usage string. |
| `get-task-allow` | no | A Release build does not carry it, and it would let any process attach. |

The programs have no entitlements: they do no JIT, load no plug-in, and a launch agent has no
sandbox to open. If a later build adds an engine that needs one (a program that loads a signed
plug-in, say), it is added to that program's signature alone, with a line in this table.

### Attestation, and why the identity matters

The engine decides whether a local peer is the signed UI with the requirement `anchor apple
generic and identifier "<app id>" and certificate leaf[subject.OU] = "<engine's team>"`, and
reads the engine's team from the engine's own signature after checking that it is `anchor apple
generic` with the identifier of its label (`ui_attestation.rs`). Today only `pane.experiment.*`
asks for a signed UI. A Developer ID build satisfies both ends because the app, the engine and
every program are signed by one team; an ad-hoc build has no team, so those methods refuse the
ad-hoc app, which is the right answer for a build nobody should run. This is the reason
`SOYEHT_DEVELOPMENT_TEAM` is required with the identity and is checked in the signature of every
program after signing.

### How the engine inside the bundle is started

**Chosen: the app runs the bundled command line, `soyeht service install --programs
<its own Contents/MacOS>`, when the person agrees, and every time it starts after that.** The
service seam stays the only writer of a service definition (ADR 0114, contract 13), and the app
stays a client: it spawns `soyeht` and reads its JSON, it does not register anything in its own
code.

**SMAppService was evaluated and not chosen for now.** `SMAppService.agent(plistName:)` is
Apple's supported way: the property list lives inside the signed bundle
(`Contents/Library/LaunchAgents`), names its program with `BundleProgram`, the item appears in
System Settings, Login Items under the app's own name, and macOS removes it when the app is
removed. Against it, for this product, today:

1. **The property list is static and signed, and ours cannot be.** launchd does not expand `~`
   or variables in `StandardOutPath` or in arguments. Contract 13 requires `--data-dir` to be
   written out and the log to be `logs/<component>.log` in the person's data directory, which
   differ per user. With a static list the engine and the supervisor would have to open their own
   log and compute their own data directory: a change to two security-sensitive programs, and a
   different definition on the Mac than on Linux and Windows.
2. **Two writers of one label.** `soyeht service status|uninstall`, `soyeht uninstall
   [--delete-data]`, `soyeht diagnostics`, the Dev installer and the end-to-end rig all read and
   write `~/Library/LaunchAgents/<label>.plist`. An item registered through the system's
   background-task database under the same label would collide with it, and every one of those
   commands would need a second branch for the Mac.
3. **It cannot be exercised here.** Registration needs an app signed with a Developer ID, a graphical
   session and the person's approval in System Settings, none of which this change has or may
   use on the owner's Mac. The seam path is the one the gate already drives against real
   launchd agents. I registered nothing, by either route, to write this ADR.

What this gives up, stated plainly:

- **Attribution.** A launch agent whose program sits in an app bundle is listed in Login Items by
  macOS; whether it is listed under the app's name or the developer's, and which process privacy
  prompts are attributed to, was not verified (no Developer ID, no registration).
- **Removal.** Dragging the app to the Trash leaves the agent registered. Its `KeepAlive` is
  conditioned on the program's existence (see the amendment), so launchd does not retry a program
  that is gone. `soyeht uninstall` (ADR 0118) is still the removal.
- **Tamper evidence.** The property list in `~/Library/LaunchAgents` is the person's own file,
  not part of the signature.

**To revisit** with a Developer ID in hand: render the same definition as a bundle property list
(the renderer is one function) and register it with `SMAppService` only if items 1 and 2 are
answered, the first by giving the programs a way to open their own log and find their data
directory with no arguments.

**First launch.** The app asks once: "Keep Soyeht running in the background?" (an alert on the
main window, shown only when the engine is not reachable and nothing is registered). "Turn on"
runs `soyeht service install`; "Not now" is remembered and the setup list keeps a "Start Soyeht"
button, so a no is never a dead end. This follows contract 13, section 3: the app registers only
when asked, because macOS tells the person about every new background item. Nothing is
registered silently.

**A moved app does not leave a stale path.** The registered definitions name the bundle's path.
Three things keep that true:

1. At every launch the app asks `soyeht service status --programs <its own folder>`. Each
   service answers with `current`: whether its definition on disk is, byte for byte, what
   `install` would write for that folder. **The rule: the app runs `service install` only when a
   service is registered and not current.** So an app moved from `~/Downloads` to `/Applications`
   repairs itself the first time it is opened there; and an app that did not move does nothing,
   which matters in two ways. A person who ran `soyeht service stop` does not find the services
   started again by the next launch (they stay registered, stopped, until the next login or
   `service start`), and macOS is not asked to announce a background item at every launch.
   The seam backs this up: `write_definition` leaves a regular file that already holds the same
   bytes untouched (a link is still replaced by a file). The install itself, when it does run, is
   idempotent (contract 13, section 3): it leaves a running program running and loads a
   registered-but-idle agent again with the new path.
   A registration that lost one of its two services (a deleted file, an install that stopped half
   way) counts as stale and is repaired the same way; one with nothing registered is a first run.
   **Registered and right, but not answering** (a stop by the person, a crash, a job the system did
   not load again): the app does not start anything at launch, since the stop must be respected,
   and the setup list offers "Start Soyeht", whose action is `install` (it loads an idle job and
   leaves a running one alone).
2. The app **refuses to register from a disk image or from an App Translocation path**
   (`/Volumes/...`, `.../AppTranslocation/...`), the two places a freshly downloaded app runs from
   before it is moved. The setup list says "Move Soyeht Next to your Applications folder and open
   it from there".
3. A program already running keeps its old path until it stops (a loaded agent keeps the text it
   was loaded with). If it crashes before the app is opened again, launchd cannot restart it; the
   next launch of the app repairs that, and so does the next login.

An app that is **updated in place** keeps its path, so nothing is stale, but the running engine
and supervisor are the old programs until they stop. Restarting them after an update is the update
check's job (ADR 0122), because stopping the supervisor ends the person's terminals.

### Code changes

- `apps/shared/.../BundledEngineService.swift` (macOS only): `BundledEngineService` (the
  command, its arguments, the stable-place rule), `ProcessRunner` and `EngineServiceSetup`
  (the phases above). Its tests run no program.
- `SoyehtMacApp.swift` creates and injects `EngineServiceSetup` once, asks the question, and
  uses `ActiveProfile.windowTitle` (the window said "Soyeht · Dev" for every profile).
  `ReadinessChecklistView` shows the "Start Soyeht" row.
- A copy with no `soyeht` in its bundle (every development build) does none of this: the Dev
  installer registers its services.
- `scripts/profile_programs.py` holds the signing identifiers, now used by `install_dev.py` and
  the packaging script alike; `scripts/package_common.py` holds the error, the SHA-256 and the
  command runners that `package_mac.py` and `package_linux.py` share.
- `soyeht service status --programs DIR` adds `current` to each service (`Manager::is_current`).

## Consequences

- One command makes a signed, notarized, stapled image from a clean checkout, given three
  environment variables, and the same command with none makes a complete ad-hoc image to test
  everything else.
- The Mac app needs no installer: opening it and saying yes starts the engine at login.
- `Soyeht Next` and `Soyeht` differ (file and window). Both carry the product name.
- The package is Apple silicon only.

## Not verified

- **Nothing was opened, installed or registered.** The app was not launched, no service was
  registered and `launchctl` was not run, by design (it is the owner's working Mac). The first
  launch question, the registration it triggers and the repair after a move are covered by the
  unit tests of `EngineServiceSetup` and of the CLI's seam, and by the manual step below.
- **Developer ID signing and notarization were not run**: there is no Developer ID. The calls are
  the documented ones and are tested as command lines; `notarytool`'s answer, `stapler` and
  Gatekeeper's acceptance of a notarized build need the first real run. An ad-hoc build is, as
  expected, rejected by `spctl`.
- **The hardened runtime on the app was not run**, so a crash that only a launched app shows
  (a missing entitlement for a code path I did not find) cannot be excluded. The dictation engine
  that downloads a model is the one to try first.
- **Login Items attribution** and the privacy prompts of the programs a launch agent starts.

### Manual check for the owner (needs a Mac that is not this one, or a spare account)

1. Mount the image, drag `Soyeht Next` to Applications, open it from there.
2. Answer "Turn on". Expect the setup list to turn green within seconds; `soyeht service status`
   (the one in the bundle) reports both services registered and running.
3. Quit the app, move it to another folder, open it, and run `soyeht service status` again: the
   programs listed by `launchctl print gui/$(id -u)/com.soyeht.next.engine` name the new folder.
4. Run `soyeht uninstall` from the bundle, then drag the app to the Trash.

## Release steps once a Developer ID exists

```sh
export SOYEHT_MAC_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAM_ID>)"
export SOYEHT_DEVELOPMENT_TEAM=<TEAM_ID>
xcrun notarytool store-credentials soyeht-notary   # once; asks for the Apple ID, team and an app password
export SOYEHT_NOTARY_PROFILE=soyeht-notary
uv run python scripts/package_mac.py --release
```

## Amendment: an agent lives only while its program does

The removal gap above is closed in the service seam, not in the app. The launchd property list now
says `KeepAlive` = `{ PathState = { "<program>" = true } }` instead of `<true/>`: launchd restarts
the program however it ended, but only while the file exists, and starts it again when the file
reappears. `launchd_start` returns `ProgramMissing` when the registered program is gone, as the
Linux side does (ADR 0121). Tests cover the rendering (including a path with markup in it), the
program read back from a written property list, and a start with the program removed.

**Untested: an update.** Whether `PathState` re-arms and starts the program again when the bundle
is replaced by a rename (how an update, or a drag over the old copy, does it) was not tested and
is not promised. The recovery that does not depend on it is the app's "Start Soyeht" offer, shown
whenever the services are registered and right but the engine does not answer; its action is
`service install`, which loads an idle agent.

**Not verified.** The `PathState` semantics are launchd's, from its documentation (`launchd.plist`);
no agent was loaded to see launchd obey them, because that registers services on the owner's Mac.
The manual check is in contract 13, section 5. An agent already loaded with the old `<true/>`
keeps it until it is installed again, which the app does the first time it finds the definition
stale (the new text differs from the file on disk, so `current` is false).
