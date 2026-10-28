# ADR 0115: One product version, kept in the contracts and written by one script

- Status: accepted
- Date: 2026-10-28

## Context

Soyeht will launch on the Mac, Windows and Linux together, with installers and updates. An
update check compares the version a build carries with the version on offer, so every build,
and every program inside it, must say the same number, and the Mac and iOS apps need a build
number that only ever goes up (`CFBundleVersion`).

Before this change the number `0.1.0` was written, by hand, in:

- `[workspace.package] version` of `engine/Cargo.toml` and of `apps/desktop/Cargo.toml` (every
  crate inherits it with `version.workspace = true`), and the entries of the workspace crates
  in the two `Cargo.lock` files;
- `contracts/typescript/package.json` and `package-lock.json` (twice);
- `MARKETING_VERSION` in the Mac and in the iOS Xcode project (two configurations each);
- the `clientVersion` of the `hello` that the Mac client and the iPhone client send, as a
  string literal in `apps/shared` (`EngineClient.swift`, `DeviceIdentity.swift`).

All of these agreed, by luck: nothing checked them. What derives from them was already sound:
the engine reports `CARGO_PKG_VERSION` in `welcome.engineVersion` and in its build record, the
desktop client sends it in `hello` and shows it in Preferences, and the Windows version
resource takes its numbers from the same variables. The Mac and iOS `Info.plist` files take
`CFBundleShortVersionString` and `CFBundleVersion` from the Xcode settings (the Mac plist
leaves them to Xcode). The build number was `1` in all four configurations, with no rule.
`scripts/install_dev.py` and `scripts/check_mac.sh` stamp nothing: the Dev installer copies
programs, and the app takes its numbers from the project.

## Decision

**The source is `contracts/release.json`**: `productVersion`, `MAJOR.MINOR.PATCH`, and an
optional `buildRevision` (0 to 99, 0 when absent; `release.py` always writes it).
It lives under `contracts/` because the version is a value that crosses the wire and the
generator already turns the contracts into every language. Three plain numbers, each below
1000, no `v`, no pre-release text: that is the form the Cargo, npm and Xcode manifests and the
Windows version resource all accept. A pre-release or a channel is a different problem and gets
its own decision.

**Generated where a file can be.** `contracts/generate.py` writes `ProductVersion.current`
into `contracts/swift/Sources/SoyehtProtocol/ProductVersion.swift`; the Mac and iPhone clients
send it as `clientVersion` instead of a literal. It also writes the revision into
`apps/desktop/packaging/windows/revision.h` (see below). The Rust side needs no other output:
Cargo hands every crate `CARGO_PKG_VERSION`.

**Written and checked where it cannot be.** A workspace manifest, a lockfile, an npm package
and an Xcode project are not generated files. `scripts/release.py` knows each place (a table
of file, pattern and expected value) and:

- `show` prints the version, the build number and the source;
- `bump major|minor|patch|revision|X.Y.Z` writes the source and every place, then runs the
  generator. An explicit version must be above the current one: a version only goes up. Any
  new version sets the revision back to 0; `bump revision` keeps the version, adds one to the
  revision and refuses to pass 99. Every file is read
  and rewritten in memory before any is written, so a place that cannot be found stops the bump
  with nothing changed. `--dry-run` lists the files and writes none;
- `sync` writes the source's values to every place without changing them (after editing the
  source by hand);
- `check` fails when a place disagrees with the source, when a crate has a version of its own
  (not `version.workspace = true`), when an `Info.plist` holds a literal where Xcode should
  fill the setting, or when `generate.py --check` fails.

`release:check` and its unit tests are `always` steps of `scripts/check_all.sh`, so a pull
request that raises one number by hand fails the fast tier. The script does not tag, push,
build or sign.

**The build number is the release as one integer**: `(major * 1_000_000 + minor * 1_000 +
patch) * 100 + revision`, so `0.1.0` revision 0 is `100000` and `1.2.3` revision 4 is
`100200304`. It is `CURRENT_PROJECT_VERSION` in the Mac and iOS projects.

The revision exists because the same version will sometimes have to be uploaded again (a
rejected binary, a signing or packaging fix, a build for testers). App Store Connect and
TestFlight reject an upload whose `CFBundleVersion` they already hold for that version, and
Sparkle orders updates by `CFBundleVersion`, so the second upload must carry a higher number
without inventing a new product version. The revision is two decimal digits, `bump revision`
raises it, and every new version starts again at 0.

Other rules were considered:

- *The commit count.* It depends on git history, so a shallow clone, a source archive or a
  history rewrite gives another number, and two machines building the same version can
  disagree. It also changes with every merge, so an installed build and the release it came
  from would differ for reasons that are not the release.
- *A counter in a file.* A second number to keep in step with the first, with nothing to say
  what it means. The revision is that counter, but it is scoped to one version and reset by
  the next, so it can never be larger than the work it names.

The number is reproducible from the source tree alone and rises with the version and, for one
version, with the revision. The parts of the version stay below 1000 and the revision below 100
so the digits never overlap, and `parse` refuses a version outside that.

**The build number stays below 2^31** (2 147 483 648), the signed 32-bit range the tools that
read it are safe with. Every release up to `20.999.999` revision 99 fits; `21.474.836` revision
47 is the last (`2147483647`). The code asserts it: `product_version.release()` raises for a
release at or above the limit, so a `bump` that would pass it fails before writing anything.

**Windows takes the revision as the fourth field of its version resource.**
`FILEVERSION` and `PRODUCTVERSION` in `soyeht.rc` are `MAJOR,MINOR,PATCH,REVISION`; the first
three come from the package version as before and the revision from the generated
`packaging/windows/revision.h` (`#define VERSION_REVISION`), which `build.rs` watches. Each
field of a Windows version resource is 16 bits wide (at most 65535); a part is below 1000 and
the revision below 100, so every field fits. This change was written on a Mac: the resource
script has not been compiled on Windows yet.

**Packaging and notarization are not here.** They need an Apple Developer account, a Windows
signing certificate and an update channel, none of which exist. `release.py` has one table of
commands (`COMMANDS`); a later `package` or `notarize` is a function that reads `current()` and
one entry in that table, with credentials taken from the environment. No command pretends to
do either today.

## Consequences

- Raising the version is `uv run python scripts/release.py bump minor`, one commit with the
  source, the places and the generated files. The pull request touches the workspace
  manifests and lockfiles, so it needs the required review.
- A new Rust crate must inherit the workspace version; a new place that carries the version is
  one entry in `PLACES` and a case in `scripts/test_release.py`.
- The fixture versions in tests (`package_version: "0.1.0"` in engine tests, the `"1"` of
  scripted `hello` calls) are test data, not product versions, and are not tracked.
- `scripts/quality/package.json` (`1.0.0`) versions the gate's own tooling, which is never
  shipped, and is left alone.
