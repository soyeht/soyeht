# ADR 0091: Engine test hooks are a feature, not a debug build

- Status: accepted
- Date: 2026-10-16

## Context

Seven behaviours of the engine existed only for tests, each behind
`#[cfg(debug_assertions)]` in a production module with a `#[cfg(not(debug_assertions))]`
stub beside it: a marker file that fails the poll of an agent, one that closes an agent's
process before its poll, one that advances a screen before a harness switch commits, one that
fails the close of a retired process, one that pauses the editor's autosave, an environment
variable that makes the skills follow folders by polling, and an environment variable that
replaces the usage sources with fixture scripts, a second host inside the production one.

The app a developer installs is a debug build. A file dropped in the data directory, or a
variable in the launch environment, therefore changed how the engine behaved for the person
using it, and the production host carried a second, fixture mode.

## Decision

**A `Hooks` port.** `soyeht_engine::hooks::Hooks` has one method per behaviour a test needs,
each with a default that does nothing: `before_agent_poll`, `before_switch_commit`,
`fail_old_close`, `pause_editor_autosave`, `poll_skills_only` and `usage_host`. `Core` holds an
`Arc<dyn Hooks>`; `Core::open_with` takes it and `Core::open` supplies `NoHooks`. Production
code asks the port and holds no test logic, no marker name and no `cfg`.

**The hooks that act exist only with the `test-hooks` feature.** `test_hooks::FileHooks` keeps
the marker files and the two environment variables, and `usage::FixtureHost` is the fixture
host (a `Host` of its own; `SystemHost` has no fixture mode). Both are compiled only with the
feature. The feature is on by no default build: `soyeht-engine` lists itself as a
development dependency with the feature, so its integration tests get it, and the extra
program `soyeht-engine-test` (`required-features = ["test-hooks"]`) is the only one that
installs `FileHooks`. `soyeht-engine` itself installs `NoHooks`. Unit tests inside the crate
inject a hook through `Core::open_with` and write no marker.

**Who uses what.** The runtime integration tests start `soyeht-engine-test`. The editor
acceptance (`scripts/e2e_editor.py`), which needs the usage fixtures, starts it too; the other
acceptances start the shipped `soyeht-engine`. `scripts/check_all.sh` builds the test engine
with `cargo build -p soyeht-engine --features test-hooks --bin soyeht-engine-test`, in the full
tier only (the fast tier runs only the unit tests of libraries and binaries, ADR 0092, and
starts no integration test or acceptance). The full tier's `cargo test` enables the feature and
also links `target/debug/soyeht-engine` from the feature-enabled library, so the full tier rebuilds the product programs (`engine:build-product`) before the acceptances run, so
they start the shipped build.

**A gate.** `scripts/check_no_test_backdoors.py` fails on `debug_assertions` in code, on a
string literal that starts with `test-` or contains `SOYEHT_TEST_`, in the `src` of an
engine crate, outside its allowlist (each entry has a reason; today only `test_hooks.rs`).
Test code is exempt. It also fails when a crate makes `test-hooks` a default feature or a
normal or build dependency enables it.

## Consequences

- A default build (`cargo build`, a release build, the installed app) has no marker file, no
  variable and no fixture host. Nothing in the data directory changes how it runs.
- A new test hook is a method of `Hooks` with a default, and an arm of `FileHooks`. It never
  adds a `cfg` to a production module.
- The runtime tests exercise the test engine, which differs from the shipped one only by the
  hooks it installs.
