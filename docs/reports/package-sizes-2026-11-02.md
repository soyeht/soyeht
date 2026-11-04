# What the shipping packages weigh, and what was cut (2026-11-02)

Every number is a byte count of a release build of commit `c3c4c75e` (the profile of
`contracts/profiles.json`, `cargo build --release --locked`, one codegen unit), except where the
host is named. The Mac and iPhone numbers come from this Mac (Apple silicon, Xcode Release, ad hoc
signature, no notarization); the Linux numbers from a x86_64 NixOS host, built with the pinned
toolchain. **No Windows installer was built**: the host has no Windows linker, so the Windows row
is an estimate from the Linux binaries (the same crates, `lzma2/max` solid compression in Inno
Setup).

## The packages

| Package | Before | After | Change |
| --- | --- | --- | --- |
| Mac disk image `soyeht-0.1.0-macos-aarch64.dmg` | 34,666,327 | 17,479,225 | -49.6% (#81, #88) |
| Mac app, installed (`Soyeht Next.app`) | 104,120 KB | 67,444 KB | -35.2% (#88) |
| iPhone and iPad app, unsigned Release (`Soyeht Dev.app`) | 63,676 KB | 61,488 KB | -3.4% (#90) |
| iPhone and iPad, zipped as an archive would be (executable stripped) | 11,449,548 | 10,342,380 | -9.7% (#90) |
| Linux x86_64 `.deb` | 17,171,820 | 17,150,460 | -0.1% (fat LTO of the window); installed 74 MB to 71 MB |
| Linux `.AppImage` | not built | not built | `mksquashfs` is not in the host's shell |
| Windows installer | not built | not built | no Windows linker here |

## What takes the space

### Mac app (104,120 KB before, 67,444 KB after)

| Part | Before | After |
| --- | --- | --- |
| `Soyeht Next` (the Swift app: SwiftUI app, `SoyehtClient`, MLX dictation engine, SwiftTerm) | 66,016,272 | 28,459,616 |
| `soyeht-engine` | 25,136,352 | 25,136,352 |
| `soyeht` (command line) | 3,841,008 | 3,841,008 |
| `soyeht-mcp` | 2,616,912 | 2,616,912 |
| `soyeht-ptyd` | 863,616 | 863,616 |
| `mlx-swift_Cmlx.bundle/default.metallib` (the dictation engine's GPU kernels) | 3,817,916 | 3,817,916 |
| fonts (Inter, JetBrains Mono x5, Newsreader x2, Nunito x3, Courier Prime x2) | 3,660 KB | 3,660 KB |
| `Assets.car`, icon, licenses, small bundles | about 0.5 MB | about 0.5 MB |
| `libswiftCompatibilitySpan.dylib` (Swift back-deployment, needed below macOS 26) | 188,176 | 188,176 |

The app executable was 66 MB because Xcode's `build` leaves the symbol table in: its `__LINKEDIT`
was 40 MB. The text is 25 MB, of which the MLX C++ is 6.7 MB, then `SoyehtClient` 2.6 MB,
`SoyehtProtocol` (the generated models) 2.5 MB and the app 2.4 MB, with the Hugging Face client
1.4 MB and SwiftTerm 1.0 MB. Dictation models are not in the app (they download on demand).

No architecture is duplicated: everything is arm64 (`lipo -archs`), and each program appears once.

### Rust programs (Linux x86_64 and Mac arm64)

| Program | Linux | Mac |
| --- | --- | --- |
| `soyeht-engine` | 28,590,760 | 25,265,136 |
| `soyeht` | 4,605,120 | 3,845,280 |
| `soyeht-mcp` | 3,029,424 | 2,614,016 |
| `soyeht-ptyd` | 952,880 | 850,448 |
| `soyeht-desktop` (the window) | 36,856,752 (34,214,800 with fat LTO) | not built there |

`cargo bloat` on the engine (text 15.7 MiB, file 33 MiB unstripped): `soyeht_engine` 4.5 MiB,
`soyeht_protocol` 1.6 MiB, `std` 2.0 MiB, serde 1.4 MiB, then the store, TLS (`rustls`, `ring`)
and `tokio` at 0.3 to 0.4 MiB each. Outside the text, `.rodata` is 9.4 MB, and 7 MB of it are the
parse tables of the tree-sitter grammars of the editor (TypeScript, Swift, Markdown, Rust, Python,
Bash and JSON). Those are the editor's languages, not a thing to drop. `.eh_frame` and the
exception tables (1.5 MB) are what `panic = "unwind"` costs, which stays.

`soyeht` and `soyeht-mcp` each carry about 1.5 MiB of `soyeht_protocol` (the generated request
and event types) and 0.2 to 0.6 MiB of serde. They are separate signed programs with their own
identifiers on the Mac, so a single multi-call program would give up that separation; it was not
tried.

The window's 36.9 MB: `std` 2.9 MiB, `zvariant` and `zbus` (D-Bus) 3.3 MiB, `naga` and `wgpu`
2.3 MiB, GPUI 1.3 MiB, the three Soyeht crates 3.8 MiB, and the image decoders 1.7 MiB. Fonts are
3.5 MB of it. `image` is already built with the six formats the window uses; the other decoders
(`exr`, AVIF) come from GPUI's own feature list and cannot be turned off without a patch of GPUI.

### iPhone and iPad (61,488 KB after)

Executable 57.0 MB unstripped (25.7 MB stripped, which an archive does), the same MLX kernels file
(3.8 MB), fonts 1.5 MB after the cut (3.7 MB before), `Assets.car` 0.4 MB. Unsigned Release
builds are what was measured; the App Store thins and compresses it again.

## What was cut

| Change | Result | PR |
| --- | --- | --- |
| Mac disk image in LZMA (`ULMO`) instead of zlib (`UDZO`) | 33.9 MB to 21.9 MB on the same app | #81 |
| Strip the Mac app's executable, keeping its symbols as a dSYM beside the package | executable 66.4 MB to 28.5 MB, image 34.7 MB to 27.5 MB; with the LZMA image 17.5 MB | #88 |
| Fat LTO for the window | window 36.9 MB to 34.2 MB, terminal reflow 4% faster | #89 |
| iPhone and iPad bundle only the fonts they register | -1.1 MB zipped | #90 |

The symbols kept aside are what keep crash reports useful: `strip` leaves the executable's UUID
alone, and `dist/symbols/soyeht-<version>-macos-aarch64.dSYM.zip` names the frames. The Rust
programs were already stripped by their profile (ADR 0127 asks the release to keep the unstripped
build).

## What was measured and left alone

| Candidate | Why not |
| --- | --- |
| Fat LTO for the engine | -1.4 MB over four programs, but the editor benchmark is 1.3% slower (open 93.0 to 94.0 ms, search 224.0 to 227.3 ms). Speed is not traded for size. |
| `DEAD_CODE_STRIPPING = YES` for the Mac app | -0.47 MB stripped, -0.12 MB compressed; the dictation engine's static registrations cannot be exercised without opening the app. |
| `opt-level` below 3, `panic = "abort"` | Forbidden by the task and by the profile's own comment. |
| Font subsetting | The Mac loads whole files, and the text a person types or a terminal prints has no known glyph set. The window's Inter cuts are subsets already. |
| Compression of the `.deb` and the Windows installer | Already `xz -9e` and `lzma2/max` solid. |
| SQLite compile options | The bundled library keeps FTS, R-tree and JSON from `libsqlite3-sys`'s defaults; they cannot be taken out without an environment override of the whole build, for about 0.3 MB. |
| `regex` Unicode tables, `url`'s IDNA data | They are behaviour (Unicode-aware matching, internationalised hosts), and under 0.3 MB together. |
| A variable Inter for the window as one static Regular | About 0.4 MB compressed; the window would need a new font file for a face that already works. |
| Intel Mac slice | Not shipped already (ADR 0120). |

## A gate for growth

A packaging script is the one place that sees the finished file, so the check is there:
`scripts/package_sizes.json` records a ceiling per package kind (`macos-aarch64.dmg`,
`linux-x86_64.deb`, ...), about 3% over what was measured, and `package_common.check_sizes`
(one helper, called last by the Mac, Linux and Windows scripts) fails when a package is over its
ceiling or is not one of the kinds in the file, and prints a warning when it is more than 10%
under, asking for the number to come down (a release must not fail because a package shrank). A
kind with `null` is not checked yet: the AppImage, the Linux arm64 `.deb` and the Windows
installers, which this host could not build; whoever builds the first one records its size. It is
not a `check_all.sh` step, because a release build is minutes and the fast tier is not; the fast
tier runs its unit test. A step that builds the Linux packages on the gate host and runs the check
would be the next one, if the owner wants it on every merge.
