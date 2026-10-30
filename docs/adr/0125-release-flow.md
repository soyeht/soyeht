# ADR 0125: One script orders a release, and nothing goes public by itself

- Status: accepted
- Date: 2026-10-30

## Context

The pieces of a release exist: the version (ADR 0115), a packager for each system (ADRs 0120,
0121 and 0123) and a signed update manifest (ADR 0122). Each runs on its own system, with its own
secrets, and nothing ordered them. Three mistakes are cheap to make by hand and expensive to undo:
publishing an image that is only ad-hoc signed, publishing a manifest whose `sequence` is far in
the future (a device never accepts a lower one again), and making a release visible to every client
before anyone looked at its files.

## Decision

`scripts/release_flow.py` has four commands, each of which can be run again:

- `prepare` raises the version with `release.py bump` (or keeps it, for the first release) and drafts
  the section of `CHANGELOG.md` from the squash commits since the last tag (`scripts/changelog.py`).
  It commits nothing. The draft carries a marker line that `collect` and `publish` refuse until a
  person has edited the entries and deleted it.
- `build` makes the Mac package here, the Linux packages on the CI host over ssh and prints the
  command for the Windows host. It refuses a checkout that is not a clean commit of `origin/main`.
  The Linux host is touched only below `~/soyeht-ci`: the commit goes to its mirror, is checked out in
  a folder named after the commit and is built by the rootless script.
- `collect` gathers the packages into `dist/`, checks that every system is present, that the Mac image
  is signed with a Developer ID certificate, notarized and accepted by Gatekeeper, that each Windows
  installer carries a signature, and that the changelog is written. It then writes the release notes,
  the signed manifest and `SHA256SUMS`.
- `publish` repeats the checks, reads the manifest as a client does (the contract's keys, the version,
  the hash and size of each file it lists, a sequence above the last published) and runs
  `gh release create` as a **draft** of `soyeht/soyeht`, targeting the commit. A draft is not the
  "latest" release, so no client sees it. Making it public (`gh release edit --draft=false`) is a
  separate command that the person runs after looking at the assets; it also creates the tag, which no
  script of ours does. `--dry-run` prints the exact commands and runs none that changes anything.

`--allow-unsigned` is the single switch for a pre-release: it accepts an ad-hoc image, unsigned
installers and a missing update key, and marks the release as a pre-release.

The release notes address in the manifest is the release page of the tag
(`release.notesUrl` of `contracts/update.json`), and the page's text is the changelog section of the
version, so the notice and the notes agree. `release_manifest.release_tag` derives the tag from that
template; no other place spells it.

### The sequence

`release_manifest.py build` refuses a sequence more than a day (`MAX_LEAD_SECONDS`) ahead of the
clock or of the last published manifest, and one that is not above the last published. The last
published sequence is read from the live address, or given (`--previous FILE`, `--last-sequence N`).
Recovery from a manifest published too high is in `docs/release.md`.

### One package per target in the manifest

The manifest lists one package per system and architecture. A Linux release has two (`.deb` and
AppImage); the manifest offers the AppImage, which runs on any distribution, and both are assets of
the release. The `.sha256` files that `package_mac.py` writes are not packages.

## Consequences

- A release is one sequence of commands, and every refusal names the file and the fix.
- The Windows host is not driven over ssh: its signing certificate is the least portable secret, so
  the person runs one command there and copies the installers back.
- The tests run the flow against stubs and build nothing (`scripts/test_release_flow.py`,
  `scripts/test_changelog.py`).
