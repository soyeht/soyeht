# ADR 0149: Every package carries the notices of its dependencies, and the gate allows only permissive licenses

- Status: accepted. Extends ADR 0120 (the Mac package), ADR 0121 (the Linux packages) and ADR 0123 (the Windows installer).
- Date: 2026-11-06

## Context

Soyeht ships the programs of three cargo workspaces (`engine/`, `apps/desktop/`, `apps/android/`), the Swift
packages the Mac and iOS apps link, the libraries of the Android app, and fonts and icons. Almost every one of
them is licensed on the condition that its license text and copyright notice travel with the binary (MIT,
BSD, ISC, Zlib, Apache-2.0 and its NOTICE files). The repository had `NOTICE` (the provider icons) and the
fonts' OFL files, and no list of the rest, no check that a new dependency carries a license a product may
ship, and nothing in the packages.

## Decision

- **One generated file, `THIRD-PARTY-NOTICES.txt`, at the root.** `scripts/third_party_notices.py generate`
  writes it: a list of every component (name, version, SPDX expression, source, and the copyright lines of its
  license file), then each distinct license or notice text once with the components that use it. Nobody edits it.
- **Where the components come from.** For the cargo workspaces, `cargo metadata --locked`: the closure of the
  normal dependencies of the members (not dev-dependencies or build tools), for every target, so the Windows,
  Linux, macOS and Android crates are all in; the license files are read from the crate's folder. For a crate
  that offers a choice (`MIT OR Apache-2.0`) the entry says which one is followed and the text of the most preferred allowed alternative that the crate
  ships is used; a crate that ships no license file gets the standard SPDX text (`scripts/third_party/spdx/`)
  and the holder its metadata names, marked as such. The license files below the top of a crate (the Unicode
  tables of `regex-syntax`, the C code of `tree-sitter` and `ring`, the protocol files of `wayland-protocols`)
  are shipped too, and so is any `LICENSE-THIRD-PARTY` file whichever license is followed. A crate that
  compiles code of another license has a `bundled` entry in the policy: `freetype-sys` builds FreeType, which
  is FTL or GPLv2, and Soyeht follows the FreeType License, whose credit line (the FreeType 2.13.2 it bundles) is in its entry. A nested license file
  classified outside the allow-list fails the check unless the crate's `bundled` entry lists that license in
  `notice_only`: `wayland-protocols-plasma` has 17 of its 29 SPDX-tagged protocol files under LGPL-2.1-or-later (the
  generated interface tables are compiled in; no LGPL library code is linked), 10 under MIT-CMU and one under
  BSD-3-Clause, and ships the four license texts with the source address
  (https://github.com/KDE/plasma-wayland-protocols). An entry of
  an MPL-2.0 component gives the address of its source (section 3.2). For the Swift packages, the pins of the four
  `Package.resolved` files against `scripts/third_party/swift.json` and the texts under
  `scripts/third_party/swift/`, which `third_party_notices.py refresh-swift --checkouts DIR` renews from
  checkouts at the pinned revisions (the nested license files of bundled sources included). For the
  Android libraries, `apps/android/app/build.gradle.kts` against `scripts/third_party/gradle.json`. The fonts and
  icons are listed in `scripts/third_party/assets.json`, with the license file already in the repository.
- **The policy is `scripts/third_party/policy.json`.** `allowed` lists the SPDX identifiers a product may ship
  (permissive licenses and MPL-2.0, whose files stay unmodified in their crates; no GPL, AGPL, LGPL alone or
  source-available license). An expression passes when one alternative of it is entirely allowed. `clarify`
  gives the license of a package whose manifest declares none, or states a combined one, each with its reason.
  `exceptions` lets a known package with a license outside the list pass, each with its reason; an exception
  that no dependency needs fails, so the list cannot go stale, and a run prints each exception it uses. `release_flow.py collect` and `publish` refuse while any exception is in the policy (`release_checks.exception_problems`), so one cannot be forgotten at a release.
- **The gate checks it.** `third-party-notices:check` (area `always`) fails when a dependency has no allowed
  license, when `swift.json` or `gradle.json` disagree with the lock files, when an exception is unused, or
  when the committed file differs from what `generate` writes. `third-party-notices:unit` runs
  `scripts/test_third_party_notices.py`. A dependency change therefore commits the regenerated file with it.
- **Every package ships it.** The Mac app: `Contents/Resources` (`package_mac.py assemble`, before signing).
  The `.deb` and the AppImage: `usr/share/doc/soyeht/`. The Windows installer: the install folder. The iOS app:
  the bundle, as resources of the Xcode project. The Android app: the APK's assets, copied by the
  `copyLicenses` task of `app/build.gradle.kts`. The packages carry `LICENSE`, `NOTICE` and the generated file.
  `package_common.LICENSE_DOCUMENTS` is the one list; a package script stops when one is missing.

## Consequences

- A new dependency fails the gate until its license is allowed and the file is regenerated; the diff of
  `THIRD-PARTY-NOTICES.txt` shows the reviewer what was added.
- The file is about 560 KB (about 100 KB compressed), under the 1 MB limit of `check_repo_files.py`: the terms of the common licenses
  are kept once and the copyright lines go with each component. It grows with the dependency graph.
- The Gradle side lists the libraries the build file declares and one line for the transitive AndroidX, Kotlin
  and Compose libraries (all Apache-2.0); it is not resolved from a Gradle dependency report.
- GPL-3.0-or-later code was in the desktop graph: Zed's `zlog`, `ztracing` and `ztracing_macro`, reached
  through `gpui -> sum_tree -> ztracing -> zlog`, at the pinned revision of GPUI (Zed relicensed the three
  crates under Apache-2.0 on 2026-09-01, after that revision; moving the pin would take unrelated changes
  of GPUI with it). Of them GPUI's `sum_tree` uses only the `#[instrument]` attribute, which does nothing in
  a build without Tracy. `apps/desktop/crates/ztracing` is an Apache-2.0 procedural macro with that one
  attribute, written from that use, and `[patch."https://github.com/zed-industries/zed"]` in
  `apps/desktop/Cargo.toml` puts it in place of Zed's crate. `ztracing_macro` and `zlog` (and
  `tracing-subscriber` with the crates only it brought) leave the build and the lockfile; `cargo tree
  --target all -i zlog` finds nothing. The Android workspace has no GPUI. `policy.json` has no
  `exceptions`, so a GPL crate that comes back fails the gate. When the GPUI pin moves to a revision after
  the relicense, the patch and the crate can go.
