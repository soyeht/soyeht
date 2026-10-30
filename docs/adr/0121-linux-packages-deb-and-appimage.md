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
`dpkg-deb` and its bytes depend only on the commit (`SOURCE_DATE_EPOCH`, else the commit time).
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

**A unit whose program is gone does nothing.** `soyeht-service` now renders
`ConditionPathExists=<program>` in the `[Unit]` section. Without it, a package removed (or a copied
folder deleted) while the unit file stays in the person's home would fail and restart every
second for ever (`Restart=always`, no start limit). With it, systemd skips the start without a
failure. This is the safety net, not the removal.

**Removal.** The data stays unless asked, as everywhere (ADR 0118).

- `.deb`: `soyeht uninstall` (as the person, before `apt remove`) stops and unregisters the units;
  `apt remove`/`purge` removes the programs and never touches `~/.local/share/soyeht-next`. The
  `prerm` prints that instruction on `remove`.
- AppImage: `AppImage soyeht uninstall [--delete-data]` runs the same command and, only when it
  succeeded, deletes the copied programs. The AppImage file itself is deleted by the person.

**Dependencies are read from the programs, not remembered.** The script reads the newest glibc
symbol version in the programs and writes `libc6 (>= X)`; it reads `DT_NEEDED` and fails on a
library that `LINKED_PACKAGES` does not name. What the window loads at run time is declared by
hand: `Depends` for what it cannot open without (`libvulkan1`, `libxkbcommon0`,
`libxkbcommon-x11-0`, `libwayland-client0`, `libxcb1`, `libfontconfig1`) and `Recommends` for what
serves something optional or may come in another form (`mesa-vulkan-drivers`, GTK 3 and WebKitGTK
4.1 for the web pane). The AppImage bundles no library: the graphics stack and the web view must be
the machine's own, and the notes in `docs/platforms.md` list them.

**Portability is the build base.** A binary needs at least the glibc of the machine that built
it. The script refuses a build that needs more than `--max-glibc` (default 2.35, Ubuntu 22.04), so
a release is built in that base (`docs/platforms.md`, "Linux packages"). `--max-glibc any` is for a
build that only has to run where it was built.

**The launcher and metadata are shared.** `com.soyeht.next.desktop` and the AppStream file
`com.soyeht.next.metainfo.xml` (id `com.soyeht.next`; its release line is written by the script from
the version and the commit time) are installed by both formats.

## Consequences

- Upgrading a package or an AppImage replaces the programs on disk but not a running engine or
  supervisor; `soyeht service stop` and `start` (or the next login) pick up the new build.
- A `.deb` removed without `soyeht uninstall` leaves the engine running until it stops or the
  person logs out, and leaves inert unit files that `soyeht uninstall` (reinstalling the package
  first, or `systemctl --user disable`) can remove.
- The `.deb` copyright file names the project licence only; the licences of the statically linked
  Rust dependencies are not listed yet.
- Not done: signing of packages or a repository, an update check (ADR 0122), RPM, Flatpak, a menu
  entry for an AppImage (a tool such as AppImageLauncher adds it), and a prompt in the window to
  register the login services.
