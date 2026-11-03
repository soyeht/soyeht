# ADR 0121: The Linux app ships as a .deb and an AppImage

- Status: accepted
- Date: 2026-10-29

## Context

Linux had a launcher entry and icons (`apps/desktop/packaging/linux`) and nothing a stranger could
install. A release is one folder of programs (`soyeht-desktop`, `soyeht-engine`, `soyeht-ptyd`,
`soyeht-mcp` and the command line, `soyeht`). Three earlier decisions shape how that folder may
be packaged:

- the window starts the engine and the supervisor from the folder of its own executable (ADR 0102);
- the login services are systemd user units that hold the absolute path of each program
  (ADR 0114), written by `soyeht service install --programs DIR`;
- the web view and the graphics stack are loaded at run time (ADR 0110), so no linker records them.

An AppImage is mounted at a different path on every run and unmounted when it exits, and a
package runs its scripts as root, so neither can simply run `soyeht service install`.

## Decision

**One script, two formats, from release builds.** `scripts/package_linux.py` builds both cargo
workspaces in release mode with `SOYEHT_BUILD_PROFILE=prod` (the production profile: application id
`com.soyeht.next`, units `soyeht-next-engine.service` and `soyeht-next-ptyd.service`, data
directory `soyeht-next`) and writes `dist/soyeht-<version>-linux-<arch>.deb` and `.AppImage` for
`x86_64` and `aarch64`. The version is `contracts/release.json` and nothing else. `dist/` is
ignored. A package of one architecture is built on that architecture. The `.deb` is written by the
script in Python (an `ar` of `debian-binary`, `control.tar.xz`, `data.tar.xz`), so it needs no
`dpkg-deb`, and given the same programs it is byte for byte the same (owner root, every time
`SOURCE_DATE_EPOCH`, else the commit time). The programs themselves are as reproducible as the
build that made them: the rootless build base installs `rustup` over the network and its
Ubuntu packages are not pinned.
The AppImage is a pinned type 2 runtime (checked by sha256) followed by a `mksquashfs` image.

**The programs never live in a path that moves.**

- `.deb`: `/usr/lib/soyeht/` holds the five programs; `/usr/bin/soyeht` and
  `/usr/bin/soyeht-desktop` are links to it, so `soyeht service install` (which reads its own real
  folder) writes `/usr/lib/soyeht` into the units. The engine, the supervisor and the MCP server
  are not on the path.
- AppImage: its `AppRun` copies `usr/lib/soyeht` out of the image to
  `$XDG_DATA_HOME/soyeht-next-appimage/programs` (default `~/.local/share`) the first time each
  build runs, and runs the window, the command line and everything they start from there. The
  copy is made beside the old one and swapped in with renames, under a lock, so a running engine
  keeps its files and two windows copy once. Units written by `soyeht service install` through
  the AppImage therefore point at that folder, which survives the AppImage being moved, renamed
  or upgraded (a new build refreshes the folder in place). The folder carries "next", like every
  machine identifier of the rewrite (ADR 0112), and is outside the data directory, so
  `--delete-data` does not delete programs that are running.

**Nothing registers the login services by itself.** A `.deb` script runs as root and cannot reach
the account of each person; registering silently would also break the consent rule of contract
13. The first run works without registration (ADR 0102: the window starts the engine). A person
who wants Soyeht running at login runs `soyeht service install` (`AppImage soyeht service install`
for an AppImage). The `.deb` does not enable a system-wide unit, since the unit must carry the
person's data directory.

**A unit whose program is gone does nothing, and says so when asked to start.** `soyeht-service` now
renders `ConditionPathExists=<program>` in the `[Unit]` section. Without it, a package removed (or a
copied folder deleted) while the unit file stays in the person's home would fail and restart every
second for ever (`Restart=always`, no start limit). With it, systemd skips the start without a
failure. Because a skipped unit makes `systemctl start` and `enable --now` exit 0, the seam checks the
program itself: `service install` already refused a missing program, and `service start` now reads
the program back from the unit's condition line and fails with the typed `ProgramMissing` error
instead of reporting a success that started nothing.

**Removal.** The data stays unless asked, as everywhere (ADR 0118).

- `.deb`: `soyeht uninstall` (as the person, before `apt remove`) stops and unregisters the units;
  `apt remove`/`purge` removes the programs and never touches `~/.local/share/soyeht-next`. The
  `prerm` prints that instruction on `remove`.
- AppImage: `AppImage soyeht uninstall [--delete-data]` runs the same command and, only when it
  succeeded, deletes the copied programs. The AppImage file itself is deleted by the person.

**Dependencies are read from the programs, not remembered.** The script reads the newest glibc
symbol version in the programs and writes `libc6 (>= X)`; it reads `DT_NEEDED` and fails on a
library that `LINKED_PACKAGES` does not name. What the window loads at run time is declared by
hand: `Depends` for what it cannot open without (`libvulkan1`, `libwayland-client0`) and
`Recommends` for what serves something optional or may come in another form (`mesa-vulkan-drivers`,
`libegl1` and `libwayland-egl1` for the OpenGL fallback, GTK 3 and WebKitGTK 4.1 for the web
pane). The tools the Setup page lists as required (`git`, `nodejs`, `npm`) are `Recommends` too:
the window opens without them, and a machine may have them in another form. `curl` stays in
`Depends`: the window runs it for the update check (ADR 0122) and the engine's usage host runs
it. The libraries the window links (`libxcb1`, `libxkbcommon0`, `libxkbcommon-x11-0`) come from
the first rule. The AppImage bundles no library: the graphics stack and the web view must be
the machine's own, and the notes in `docs/platforms.md` list them.

**Portability is the build base.** A binary needs at least the glibc of the machine that built
it. The script refuses a build that needs more than `--max-glibc` (default 2.35, Ubuntu 22.04), so
a release is built in that base (`docs/platforms.md`, "Linux packages"). `--max-glibc any` is for a
build that only has to run where it was built. `scripts/package_linux_rootless.sh` makes the base
without root: the Ubuntu 22.04 base image, entered with `bwrap`, with the build libraries and
`rustup` installed inside it, then `package_linux.py` run on the committed sources.

**The launcher and metadata are shared.** `com.soyeht.next.desktop` and the AppStream file
`com.soyeht.next.metainfo.xml` (id `com.soyeht.next`; its release line is written by the script from
the version and the commit time) are installed by both formats.

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

## Consequences

- A skipped unit is not retried. An engine that exits while the programs are briefly missing (for
  example during the swap of `AppRun`, or a developer relinking the folder) stays stopped until the
  next `service start` or login, where `Restart=always` alone would have started it again a second
  later. That is the price of not restarting for ever a service whose program was removed with its
  package, which fills the log and burns a processor with nothing the person can see; the swap itself
  leaves the old programs in place until the new ones are complete, so the gap is short, and
  `soyeht service start` now says plainly when the programs are not there.
- Upgrading a package or an AppImage replaces the programs on disk but not a running engine or
  supervisor; `soyeht service stop` and `start` (or the next login) pick up the new build.
- A `.deb` removed without `soyeht uninstall` leaves the engine running until it stops or the
  person logs out (the Mac programs end themselves when their file is gone, ADR 0130; Linux and
  Windows do not yet, because their upgrades remove the file for a while), and leaves inert unit files that `soyeht uninstall` (reinstalling the package
  first, or `systemctl --user disable`) can remove.
- The `.deb` copyright file names the project licence only; the licences of the statically linked
  Rust dependencies are not listed yet.
- Not done: signing of packages or a repository, an update check (ADR 0122), RPM, Flatpak, a menu
  entry for an AppImage (a tool such as AppImageLauncher adds it), and a prompt in the window to
  register the login services.

## Verification

On Ubuntu 26.04 arm64 (a VM), for the packages built in the Ubuntu 22.04 base (glibc 2.35):
`dpkg-deb -I/-c`, `apt-get -s install`, a full `dpkg` install, remove and purge into a private root
(maintainer scripts included), `desktop-file-validate` and `appstreamcli validate` all pass; the
AppImage opens the window (a screenshot shows the empty workspace, the engine ready), the engine and
supervisor it starts run from `~/.local/share/soyeht-next-appimage/programs`;
`soyeht service install` through the AppImage writes units that name that folder and survive the AppImage
being renamed and moved; hiding the programs makes systemd skip the unit (`ConditionResult=no`, zero
restarts); `soyeht uninstall` through the AppImage unregisters the units, removes the copied programs and
keeps the data, and `--delete-data --yes` deletes the production data and leaves the Dev folder inside it.
Units written by the `.deb` layout name the real `usr/lib/soyeht` folder through the `usr/bin` link, and are
skipped after the package files are removed. On x86_64 (Ubuntu 22.04 base on a gate host) the packages
build, `dpkg-deb`, a private `dpkg` install, remove and purge work, and the AppImage's command line,
engine and supervisor run.

Not verified: `sudo apt install ./soyeht.deb` (the VM account has no passwordless `sudo`, so the install
went into a private root); a window on x86_64 (no display there); a systemd user manager on x86_64;
installing on any distribution other than Ubuntu; the AppImage on a system without FUSE (only its
`--appimage-extract-and-run` path on x86_64); package signing.
