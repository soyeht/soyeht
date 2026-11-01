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
| `SOYEHT_NOTARY_PROFILE` | notarizing the app and the image | a keychain profile made once with `xcrun notarytool store-credentials <name>`; the variable holds the name |
| A code-signing certificate for Windows | signing the installers and their programs | on the Windows host: `SOYEHT_WINDOWS_SIGN_THUMBPRINT` (a certificate in the store), or `SOYEHT_WINDOWS_SIGN_AZURE_DLIB` with `SOYEHT_WINDOWS_SIGN_AZURE_METADATA` (Azure Artifact Signing). `docs/platforms.md`, "Windows packages", has the rest |
| The update signing key | signing `update-manifest.json` | `SOYEHT_UPDATE_SIGNING_KEY` in the environment of the Mac that collects; the key itself in a password manager, never in a file of this repository |
| `gh` logged in with write access to `soyeht/soyeht` | the draft release | `gh auth login`, or `GH_TOKEN` in the environment |
| The Linux CI host | the `.deb` and the AppImage | `bignix`: a clone at `~/soyeht-ci/repo` whose `origin` is the mirror `~/soyeht-ci/mirror.git`, Nix, and the settings of `docs/platforms.md`, "A Linux gate host". The release uses only `~/soyeht-ci` |
| A Windows host | the installers | Windows with the build tools of `docs/platforms.md`, "Windows packages", Inno Setup 6 and `uv`. A VM through VMPal does for a dry run (below) |

### The update key, before anything is built

Every build carries the public half of the update key (`signature.publicKeys` of
`contracts/update.json`), and a client without it shows "Update checks are not set up in this
build". So the key is made and merged before the packages that will carry it:

```sh
uv run python scripts/release_manifest.py keygen      # prints the seed on stdout, the public entry on stderr
```

Store the seed (the first line) as `SOYEHT_UPDATE_SIGNING_KEY`. Add the entry (`{"id": "release-1",
"key": "..."}`) to `signature.publicKeys` in `contracts/update.json`, run `uv run python
contracts/generate.py`, and merge that change. A lost key cannot be replaced for installed builds:
add the next key to the contract in a release that is signed with the old one, and only then retire
the old one. A leaked key is the reverse: ship a build that drops it, and sign with the new one.

## The steps

The first release of 0.1.0 keeps the version (`prepare current`). Later ones name the part to raise.

1. **Prepare.** On a clean `main`:

   ```sh
   uv run python scripts/release_flow.py prepare current     # or minor, patch, major, X.Y.Z, revision
   ```

   It runs `release.py bump` and drafts the section of `CHANGELOG.md` from the squash commits since
   the last tag (`scripts/changelog.py`). Edit the entries so they read well for a person who uses
   Soyeht, delete the line that starts with `<!-- draft:`, commit the bump and the changelog, and
   merge them as a pull request. `--dry-run` shows what it would do. Running `prepare` again replaces a
   section that still has its draft line, and refuses one you have edited (`changelog.py draft
   --replace --force` writes over it on purpose).

2. **Build.** From a clean checkout of the merged commit of `main` (the script refuses any other):

   ```sh
   uv run python scripts/release_flow.py build --mac --linux
   ```

   The Mac package is built here under `nice` with `package_mac.py --release` (it needs the three
   variables above). The Linux packages are built on the CI host: the commit is pushed to the mirror
   (`SOYEHT_RELEASE_RECEIVE_PACK` names the host's `git-receive-pack` when its ssh sessions have no
   `git`), checked out in `~/soyeht-ci/release/<commit>`, built by `package_linux_rootless.sh` and
   copied back to `dist/`. The host is `bignix`, or `--linux-host`, or `SOYEHT_RELEASE_LINUX_HOST`.

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
   into `dist/` and checks that all five are there (the Mac image, the `.deb`
   and AppImage, the two installers), that the image is signed with a Developer ID certificate,
   notarized and accepted by Gatekeeper, that each installer carries a signature, and that the
   changelog section is written. Then it writes the release notes (`release-notes.md`), the signed
   `update-manifest.json` and `SHA256SUMS`. The manifest offers the AppImage to Linux clients, and its
   sequence is the clock, checked against the manifest the clients read now (below).

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
is no manifest), `--without windows` (or `linux`, `macos`) leaves a system out, and the release is
marked as a pre-release. A pre-release is never "latest", so no client is told. Everything else is
the same.

### A dry run of the whole flow

`build --dry-run` and `publish --dry-run` print the commands and run none that changes anything; they
list what would stop the real run and exit 1 when something would. For the Windows step on a Mac
without a Windows machine, VMPal runs the installer script in a VM (`docs/platforms.md`, "Running
them through VMPal"); an unsigned installer is fine for that and for `--allow-unsigned`.

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
Run `check-expiry` on a schedule (every week from the calendar or from a CI job that fails the run), and
renew when it fails. A manifest that has expired already can be renewed the same way.

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
