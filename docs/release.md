# Releasing Soyeht

A release is the packages of the three systems, a checksum list, a signed update manifest and the
release notes, published as one GitHub release of `soyeht/soyeht`. Each packager runs on its own
system (ADRs 0120, 0121 and 0123); `scripts/release_flow.py` orders them and refuses to go on when
something is missing, unsigned or inconsistent (ADR 0125). It never creates a tag, never publishes
by default and reads every secret from the environment.

```
prepare  ->  (edit, commit, merge)  ->  build  ->  collect  ->  publish (draft)  ->  check  ->  make it public
```

## What you must have

Before the first release, once:

| What | For | Where it lives |
|---|---|---|
| A "Developer ID Application" certificate and its private key | signing the Mac app and image | the login keychain of the Mac that builds |
| `SOYEHT_MAC_SIGNING_IDENTITY`, `SOYEHT_DEVELOPMENT_TEAM` | `package_mac.py --release` | the environment (the identity is the certificate's name or SHA-1; the team is its ten-character id) |
| `SOYEHT_NOTARY_KEY_FILE`, `SOYEHT_NOTARY_KEY_ID`, `SOYEHT_NOTARY_ISSUER` | notarizing the app and the image | an App Store Connect API key: the path of its `.p8` file (outside the repository; `package_mac.py` refuses one that other users can read), its key identifier and the team's issuer identifier, all in the environment. Alternatively `SOYEHT_NOTARY_PROFILE`, the name of a keychain profile made once with `xcrun notarytool store-credentials`; the profile wins when both are set |
| A code-signing certificate for Windows | signing the installers and their programs | on the Windows host: `SOYEHT_WINDOWS_SIGN_THUMBPRINT` (a certificate in the store), or `SOYEHT_WINDOWS_SIGN_AZURE_DLIB` with `SOYEHT_WINDOWS_SIGN_AZURE_METADATA` (Azure Artifact Signing). `docs/platforms.md`, "Windows packages", has the rest |
| The update signing key | signing `update-manifest.json` | `SOYEHT_UPDATE_SIGNING_KEY_FILE` (a path) or `SOYEHT_UPDATE_SIGNING_KEY` (the value) in the environment of the Mac that collects; see "The update key" below. Never a file of this repository |
| `gh` logged in with write access to `soyeht/soyeht` | the draft release | `gh auth login`, or `GH_TOKEN` in the environment |
| The Linux x86_64 CI host | the x86_64 `.deb` and AppImage | an ssh host you name (`--linux-host`, or `SOYEHT_RELEASE_LINUX_HOST`; the repository has no default): a clone at `~/soyeht-ci/repo`, the mirror `~/soyeht-ci/mirror.git` (the release commit is fetched from it, whatever the clone's `origin` is), a git at `~/soyeht-ci/git/bin` when the login shell has none, Nix, and the settings of `docs/platforms.md`, "A Linux gate host". The release uses only `~/soyeht-ci` |
| A Linux aarch64 (arm64) CI host | the aarch64 `.deb` and AppImage | the same set-up on an arm64 Linux machine, named by `--linux-aarch64-host` or `SOYEHT_RELEASE_LINUX_AARCH64_HOST` (and `SOYEHT_RELEASE_LINUX_AARCH64_RECEIVE_PACK` for its `git-receive-pack`, as below). The packager builds for the architecture of the machine it runs on and does not cross-build, so the x86_64 host cannot stand in for it; the build script stops at once on a host of the other architecture. The Ubuntu 26.04 arm64 VM that verified ADR 0121 is such a host once it has the set-up above. The repository names no host. The size ceilings of the aarch64 packages in `scripts/package_sizes.json` are `null` (not checked) until the first build measures them |
| A Windows host | the installers | Windows with the build tools of `docs/platforms.md`, "Windows packages", Inno Setup 6 and `uv`. A VM through VMPal does for a dry run (below) |

### The update key

The update key signs `update-manifest.json` and belongs to the release flow, not to any Apple
account. Its public half is `signature.publicKeys` of `contracts/update.json` (the entry `release-1`),
generated into the Rust and Swift clients, so every build carries it; a build with no public key
shows "Update checks are not set up in this build". `contracts/test_update_manifest.py` fails when
the list is empty, so a release cannot go out without one.

**Where the private half lives.** It is the 32-byte seed in base64, one line, made by
`uv run python scripts/release_manifest.py keygen` (the seed on stdout, the public entry on stderr).
On the release Mac it is `~/.config/soyeht/release/update-signing-key`, in a folder of mode 700 and
a file of mode 600, outside every checkout. The flow reads it from either place, and the value wins
when both are set:

```sh
export SOYEHT_UPDATE_SIGNING_KEY_FILE=~/.config/soyeht/release/update-signing-key   # a path; the file must be 600
# or: export SOYEHT_UPDATE_SIGNING_KEY="$(cat ~/.config/soyeht/release/update-signing-key)"
```

`release_manifest.py` refuses a key file that other users can read. `collect` and `renew` need the
key; `verify` and `check-expiry` need only the public half.

**Back it up.** There is one copy until you make a second. Put the file in a password manager (as a
secure note) and seal one more copy offline (an encrypted disk image on a drive kept elsewhere). A lost
key cannot be replaced for installed builds: they trust only the keys they carry, so every installed
copy would need a build that carries a new key, installed by hand. Never put the seed in the
repository, a pull request, a chat or a log. Check the backup by deriving the public key from it and
comparing with `contracts/update.json`:

```sh
SOYEHT_UPDATE_SIGNING_KEY_FILE=<the backup> uv run python -c "import sys; sys.path.insert(0,'scripts'); import os, release_manifest as r; print(r.key_id_for(r.signing_seed(os.environ), r.POLICY['signature']['publicKeys']))"
```

It prints `release-1` when the backup is the key the clients trust.

**The duties that come with it.**

- *Renew the manifest at least every 90 days.* A manifest expires `manifest.maxLifetimeSeconds` after it
  is signed and a device refuses an expired one. The renewal needs the key and no rebuild (see "The
  expiry" below). Run `check-expiry` every week.
- *Rotate on a schedule you choose, and at once if the seed may have been seen.* A new key is added to
  `signature.publicKeys` in a release signed by the old one; the old one is removed in a later release.
  A leaked key is the reverse: ship a build that drops it and sign with the new one. A copy that never
  updates stays exposed.
- *Keep it on the release Mac only.* Collecting a release is the one task that reads it.

**Before the first release is public.** A build that carries the key asks GitHub for the manifest once
a day, and until a release is published there is none (404), so it honestly says "Could not check for
updates"; that is the expected state, not a fault, and it ends when the first release is made public.

If the key has to be made again before the first public release (nothing has shipped, so nothing
trusts it yet), run `keygen`, replace the entry in `contracts/update.json`, run `uv run python
contracts/generate.py`, and merge that change before the packages are built.

## The steps

The first release of 0.1.0 keeps the version (`prepare current`). Later ones name the part to raise.

1. **Prepare.** On a clean `main`:

   ```sh
   uv run python scripts/release_flow.py prepare current     # or minor, patch, major, X.Y.Z, revision
   ```

   It checks everything that can refuse first (the version, the wire contract, the changelog), then
   runs `release.py bump` (not for `current`), records the wire contract of the release in
   `contracts/released/` (`release.py snapshot`, ADR 0161; commit the new files with the bump) and
   drafts the section of `CHANGELOG.md` from the squash commits since the last tag
   (`scripts/changelog.py`). `--dry-run` reports exactly what the real run will do, and a refusal
   (a wire contract that differs from the recorded one, a version that cannot be raised, a finished
   section with no entry) writes nothing, in either mode. Running `prepare` again replaces a section
   that still has its draft line. A section you have finished (no `<!-- draft:` line) is kept as it
   is, which is what the first release of 0.1.0 does when the changelog was written by hand: the
   wire contract is still recorded, and `changelog.py draft --replace --force` writes over a
   finished section on purpose. Otherwise edit the entries so they read well for a person who uses
   Soyeht, delete the line that starts with `<!-- draft:`, commit the bump, the wire contract and the
   changelog, and merge them as a pull request.

2. **Build.** From a clean checkout of the merged commit of `main` (the script refuses any other):

   ```sh
   uv run python scripts/release_flow.py build --mac --linux
   ```

   The Mac package is built here under `nice` with `package_mac.py --release` (it needs the three
   variables above). The Linux packages are built on a CI host of each architecture, one after the other:
   the commit is pushed to the host's mirror (`SOYEHT_RELEASE_RECEIVE_PACK`, and for the arm64 host
   `SOYEHT_RELEASE_LINUX_AARCH64_RECEIVE_PACK`, name its `git-receive-pack` when its ssh sessions have no
   `git`), checked out in `~/soyeht-ci/release/<commit>`, built by `package_linux_rootless.sh` and
   that architecture's `.deb` and AppImage copied back to `dist/`. The x86_64 host is `--linux-host` or
   `SOYEHT_RELEASE_LINUX_HOST`, the aarch64 host `--linux-aarch64-host` or
   `SOYEHT_RELEASE_LINUX_AARCH64_HOST`; there is no default for either, and `--linux` needs both.
   The hosts are looked up before anything is built, so a missing one fails at once. `BUILD_SHA` is written
   as soon as a step has built, so if one host fails after the other copied its packages, run
   `build --linux-arch aarch64` (or `x86_64`) again for the one that failed. `--linux-arch` implies
   `--linux`, and with no other flag builds that architecture alone.

3. **Windows.** `build --windows` prints the command; run it on the Windows host with the
   `SOYEHT_WINDOWS_SIGN_*` variables set:

   ```powershell
   git fetch origin; git checkout <commit>
   uv run python scripts\package_windows.py --require-signature
   git rev-parse HEAD | Out-File -Encoding ascii dist\BUILD_SHA
   ```

   It writes `dist\soyeht-<version>-windows-x86_64.exe` and `-aarch64.exe`. Copy them and `BUILD_SHA`
   to a folder on the Mac (`scp`, a share) for the next step.

4. **Collect.**

   ```sh
   uv run python scripts/release_flow.py collect --from ~/windows-installers
   ```

   `build` wrote `dist/BUILD_SHA` (and `dist/ad-hoc/BUILD_SHA`) after the packages were built: the commit the packages come from; `collect` and `publish` refuse
   packages of another commit (or of another version, which are never uploaded). It copies the packages
   into `dist/` and checks that all seven are there (the Mac image, the `.deb`
   and AppImage of each Linux architecture, the two installers), that the image is signed with a Developer ID certificate,
   notarized and accepted by Gatekeeper, that each installer carries a signature, and that the
   changelog section is written. Then it writes the release notes (`release-notes.md`), the signed
   `update-manifest.json` and `SHA256SUMS`. The manifest offers each Linux architecture its own AppImage, and its
   sequence is the clock, checked against the manifest the clients read now (below).

   While the Android app depends on Google's ML Kit (ADR 0157), the release notes say that the APK contains
   proprietary runtime libraries (ML Kit and Google Play services, under Google's terms, not open source): the
   changelog section carries that line, and the policy's accepted exceptions are what lets the checks pass it.

5. **Publish, as a draft.** First look at it:

   ```sh
   uv run python scripts/release_flow.py publish --dry-run
   ```

   It prints the exact `gh release create` command (a draft, for `soyeht/soyeht`, targeting the
   commit, with the notes from the changelog and every file) and anything that would stop it. Then:

   ```sh
   uv run python scripts/release_flow.py publish
   ```

   A draft is not the "latest" release, so no client reads its manifest yet.

6. **Check the draft** (on the releases page, or `gh release view v<version> --repo soyeht/soyeht`):
   download the assets into an empty folder, run `shasum -a 256 -c SHA256SUMS` and `uv run python
   scripts/release_manifest.py verify update-manifest.json --running 0.0.1 --system linux
   --architecture x86_64`, and install the image or package on a machine you can lose.

7. **Make it public.** This creates the tag `v<version>` at the commit and starts the update
   notices:

   ```sh
   gh release edit v<version> --repo soyeht/soyeht --draft=false
   ```

   Then read the live manifest as a client does:
   `curl -sL https://github.com/soyeht/soyeht/releases/latest/download/update-manifest.json -o
   live.json && uv run python scripts/release_manifest.py verify live.json`.

### A pre-release

`--allow-unsigned` (on `collect` and `publish`) is for a build to try on friends: it accepts the
ad-hoc Mac image from `dist/ad-hoc/`, installers without a signature, and no update key (then there
is no manifest), `--without windows` (or `linux`, `macos`) leaves a system out, `--without linux-aarch64` (or `linux-x86_64`)
leaves one Linux architecture out, so Linux x86_64 can ship without an arm64 host, and the release is
marked as a pre-release. A pre-release is never "latest", so no client is told. Everything else is
the same.

### A dry run of the whole flow

`build --dry-run` and `publish --dry-run` print the commands and run none that changes anything; they
list what would stop the real run and exit 1 when something would. For the Windows step on a Mac
without a Windows machine, VMPal runs the installer script in a VM (`docs/platforms.md`, "Running
them through VMPal"); an unsigned installer is fine for that and for `--allow-unsigned`.

### The Mac, once the Developer ID certificate exists

Until the owner has a "Developer ID Application" certificate and an App Store Connect API key, the
Mac package is built ad hoc (`release_flow.py build --mac --allow-unsigned`, which runs `package_mac.py` without `--release`, so
it signs ad hoc when the identity is not in the environment): it goes to `dist/ad-hoc/`, is not notarized, and only a pre-release
(`--allow-unsigned`) accepts it. The script reads the identity, the team and the notary credentials
**only from the environment** and refuses `--release` without them (ADR 0120). Nothing in this
repository creates a certificate, a key or an app record in an Apple account.

With the certificate in the login keychain of the release Mac (`security find-identity -v -p
codesigning` lists it) and the API key's `.p8` file kept outside the repository:

```sh
export SOYEHT_MAC_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAM_ID>)"
export SOYEHT_DEVELOPMENT_TEAM=<TEAM_ID>
export SOYEHT_NOTARY_KEY_FILE=<path of AuthKey_<KEY_ID>.p8>
export SOYEHT_NOTARY_KEY_ID=<KEY_ID>
export SOYEHT_NOTARY_ISSUER=<ISSUER_UUID>
uv run python scripts/release_flow.py build --mac      # runs package_mac.py --release under nice
```

`package_mac.py --release` signs, notarizes and staples both the app and the image, and checks the
result. The same steps by hand, to see a rejection's log or to repeat one step (the image is
`dist/soyeht-<version>-macos-aarch64.dmg`; the credentials are the variables above):

```sh
DMG=dist/soyeht-<version>-macos-aarch64.dmg
xcrun notarytool submit "$DMG" --key "$SOYEHT_NOTARY_KEY_FILE" --key-id "$SOYEHT_NOTARY_KEY_ID" \
    --issuer "$SOYEHT_NOTARY_ISSUER" --wait --timeout 1h        # prints "status: Accepted" and an id
xcrun notarytool log <id> --key "$SOYEHT_NOTARY_KEY_FILE" --key-id "$SOYEHT_NOTARY_KEY_ID" \
    --issuer "$SOYEHT_NOTARY_ISSUER"                            # only when it was not accepted
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"                                   # "The validate action worked!"
spctl -a -vv -t open --context context:primary-signature "$DMG" # "accepted", "source=Notarized Developer ID"
```

Then mount the image read-only and check the app inside the same way:

```sh
MOUNT=$(mktemp -d)
hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$MOUNT" "$DMG"
spctl -a -vv -t execute "$MOUNT/Soyeht Next.app"     # "accepted", "source=Notarized Developer ID"
xcrun stapler validate "$MOUNT/Soyeht Next.app"
hdiutil detach "$MOUNT"
```

`collect` repeats the signing, staple and Gatekeeper checks, so an image that fails any of them is
never put in the release.

## When something goes wrong

**The expiry.** Every manifest expires 90 days (`manifest.maxLifetimeSeconds`) after it is signed, and a
device refuses one at or past its expiry (`expired`, contract 15 section 3), so an old manifest that
someone keeps serving cannot hold a device on an old release. The same rule means that when no release is
published for 90 days, every device's update check fails (the Mac and the desktop app say to check the
date and time or to get Soyeht from the website). Renewing needs neither `dist/` nor a rebuild, and the
packages and their hashes stay exactly as published:

```sh
uv run python scripts/release_manifest.py check-expiry            # fails within 21 days of the expiry
SOYEHT_UPDATE_SIGNING_KEY=... uv run python scripts/release_manifest.py renew   # writes update-manifest.json
gh release upload v<version> update-manifest.json --clobber       # over the asset of the latest release
```

`renew` reads the manifest the clients read now, keeps its version, minimum, notes and packages, and signs
them with a new sequence (the clock, or one above the old if the clock is behind) and a full life.
Run `check-expiry` on a schedule: the workflow `.github/workflows/update-manifest-expiry.yml` does it every
week (and on demand) against the live manifest with `--allow-unpublished`, so it passes while no release
exists and fails the run once the manifest expires within 21 days. Renew when it fails. A manifest that has expired already can be renewed the same way.

**The sequence.** A device remembers the highest `sequence` it has read and refuses every lower one
(contract 15, section 3). So a manifest with a sequence far in the future would lock every later
release out of the devices that read it, for ever. `release_manifest.py build` refuses a sequence more
than a day ahead of the clock (or of the last published manifest), and one that is not above the last
published, and `collect` reads the last published manifest from the live address.

- *"not above the last one published"*: the clock is behind the one that signed the last manifest, or
  the last one was signed too high. Give `--sequence N` with N one above the last (the message names
  it).
- *"more than a day ahead"*: a sequence given by hand in the wrong unit (milliseconds). Leave
  `--sequence` out.
- *A manifest with a too high sequence was published.* The devices that read it refuse the lower
  ones; nothing lowers a device's memory. Delete the asset or the release at once, so fewer devices
  read it (GitHub serves "latest" within minutes). For the ones that did, publish the next release
  with a sequence just above the bad one: `collect --last-sequence <bad> --sequence <bad + 1>`.
  The ceiling follows the bad manifest (a day above it), so this passes, and ordinary releases
  continue once the clock is past it.
- *No network, or the release page is down*: `collect --last-sequence N` with the number from the last
  release you made, or `--first-release` when nothing was ever published.

**The release exists already.** `publish` refuses a tag that exists. A draft that is wrong is deleted
by hand (`gh release delete v<version> --repo soyeht/soyeht`), and the build is collected again. A
public release that is wrong is replaced by a new revision: `prepare revision` or `patch`, and
a manifest with a higher sequence. Do not reuse a tag.

**The notarization is rejected, or the image fails Gatekeeper.** `package_mac.py` prints the log;
nothing is collected from `dist/` until the image passes `collect`.

**A signature is missing on a Windows installer.** `collect` names the file. Sign it by running
`package_windows.py --require-signature` again on the Windows host, with the variables set.

## What the scripts do not do

They do not tag (GitHub does when the draft is made public), do not push to `main`, do not open the
pull request and do not read a secret from a file of this repository. The Linux host is touched only
below `~/soyeht-ci`. `scripts/test_release_flow.py` runs the flow against stubs: it builds nothing and
reaches no network.
