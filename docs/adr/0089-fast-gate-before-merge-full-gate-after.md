# ADR 0089: A fast gate before the merge, the full gate after it

- Status: accepted; the content of the fast tier is amended by ADR 0092
- Date: 2026-10-16

## Context

Every pull request ran all of `scripts/check_all.sh` before it merged: about 70 steps, 40
to 60 minutes, with every build from scratch in a fresh worktree. That caps the project at
about ten merges a day, and most of the time goes to steps that a change cannot affect: a
documentation fix waited for the Mac and iOS app builds, the end-to-end suites and the
editor measurement on the VM.

## Decision

**Two tiers, one list.** `scripts/check_all.sh` stays the only list of gate steps. Each step
is a `step AREA TIERS ID command...` line. The areas are `always`, `engine`, `desktop`,
`swift` (with `dictation`, the tests of `apps/dictation`), `mac_app`, `ios_app`,
`typescript`, `e2e`, `editor_perf` and `timing`. `--tier full` is the default and runs what the script
always ran. `--tier fast` runs:

- every `always` step: the forbidden-terms scan, the hardening and quality ratchets, the
  other `check_*` scripts with their unit tests, `generate.py --check` and the contracts
  tests;
- for each selected area, format, lint, a build and the unit tests (ADR 0092 narrowed this
  from the full tests). For the engine that is format and `clippy -D warnings` over the
  workspace, a workspace build, and `cargo test --lib --bins` of the changed crates and the
  crates that depend on them. The desktop runs its full format, clippy (both feature sets)
  and `cargo test --lib --bins` (amended by ADR 0092: its whole `cargo test`). Swift runs `swift format lint` and a `swift build` of
  `apps/shared`. TypeScript runs `check` and `test`.

The fast tier leaves out the integration tests, the Swift tests, the Mac and iOS app builds,
the end-to-end suites, the editor measurement on the VM and the ignored timing tests (the
areas `dictation`, `mac_app`, `ios_app`, `e2e`, `editor_perf` and `timing`, and the full
steps of the others); only the full tier runs them. A fast step that stands in for a full
one is named `<full id>-changed`, `<full id>-fast` or `<full id>-build`, and
`test_gate_list.py` checks the list: known areas and tiers, unique ids, a full step behind
every stand-in. The ratchet counts the steps that run in both tiers per area, so moving one
out of the fast tier needs a `Verification change:` line, as removing it does.

**The areas come from the paths.** `scripts/gate_plan.py` maps the files that differ from the
merge-base of `origin/main` (or `--base`) to areas, and `check_all.sh` prints the plan at the
top of the log. A change to `contracts/` (other than the Swift and TypeScript outputs),
`engine/soyeht-protocol`, the gate tooling (`scripts/`, `.github/`, `.githooks/`; not the unit tests `scripts/test_*.py`), a lockfile
or a workspace manifest selects every code area, and so does a cargo workspace that cannot
be read. A change to the scripts that run the gate (`check_all.sh`, `gate_*`, `post_merge*`,
`main_watch*`, `remote_gate*`, `e2e*`, the app build scripts) upgrades the run to the full
tier; a change to their unit tests alone (`scripts/test_*.py`) selects `always`, which runs
those tests (ADR 0092). A file that another
directory reads and Cargo does not show (the Xcode project that the engine's attestation
embeds, the fonts and icons the desktop embeds) is listed in `CROSS_READS`; a test scans the
engine and desktop sources for such reads and fails on one that is missing. A path under an engine crate selects the
engine and, through `cargo metadata`, that crate and its reverse dependencies; a crate that
the desktop workspace depends on selects the desktop too. Documentation selects only
`always` (Markdown counts as documentation only under `docs/` and `design/`, at the root and
as a README); an unknown path selects everything. The ratchet data files
(`scripts/*baseline*.json`, `scripts/*allowlist*.json`) are data, not tooling, because a
pull request that lowers a baseline must not run everything.

**The full gate runs after the merge.** `scripts/post_merge.py` runs
`check_all.sh --tier full` on a main commit in a clean worktree, against the commit's
parent, and records the result, the duration and the per-step times in
`.cache/main-health/<sha>.json`, with `latest.json` and `last_green.json`. When it is red
it reruns the failing step once on the same commit; a pass is recorded as flaky and blames
no commit. Otherwise it takes the first-parent commits since the last green main and
searches them with the failing step only (`check_all.sh --only STEP_ID`, with the step it
depends on; a binary search, which is exact for the few squash commits between two runs). A
commit that cannot run the step or cannot be checked out is skipped, and the candidates are
reported. It reports the culprit commit, the failing step and the log, and exits non-zero.
It never pushes, reverts or edits main. One run at a time (a lock), SIGTERM cleans up, and a
problem of the machine (no restricted vocabulary, a gate that cannot start) writes no record.
The coordinator session runs it after each merge or every few merges (ADR 0092 runs
it continuously, with an automatic revert of a confirmed culprit). The plan prints a
warning while `latest.json` says main is red. The editor measurement on the VM stays
separate: it is collected when the editor hash changes.

**Per-step timings.** Each step's wall time goes to `.cache/gate-timings.json` (the latest
run and the last fifty), so slow steps can be watched with `scripts/gate_timings.py top`.

**A shared build cache.** When `sccache` is on `PATH` and the caller set no `RUSTC_WRAPPER`,
`check_all.sh` sets `RUSTC_WRAPPER=sccache`, `SCCACHE_DIR` (default
`~/Library/Caches/soyeht-sccache`) and `CARGO_INCREMENTAL=0`, since sccache does not cache
incremental crates, and probes the cache first: when sccache does not start, the gate
builds without it and says so. The cache is content-addressed, so worktrees may share it;
they must not share a target directory. Cache keys include the source path, so workspace
crates of another worktree miss; dependencies hit.

**Risk-scoped review.** `gate_plan.py` prints `review: required` when the change touches the
engine core (`core*`, `dispatch*`, `server*`, `agents/` of `soyeht-engine`, and
`soyeht-store`), the protocol (`engine/soyeht-protocol`, `contracts/protocol.json`), an
editor (the Mac `Editor*` files, the desktop editor, `soyeht-editor`, the engine's `editor/`),
security-sensitive code (the platform, the supervisor, the engine's `auth`, `pairing`, `push`,
`network` and `ui_attestation`, web permissions), dependencies, ratchet exceptions or the
gate tooling; otherwise `optional`.

## Consequences

A pull request waits for the checks its paths need, so most merge in minutes. A change can
now reach main with a failure that only a full-tier step finds. The rule that keeps this
safe is that red main comes first: it is fixed forward or reverted within the hour, and no
other change to the affected area merges while it is red. Cutting an area from the fast
tier is a change to `check_all.sh`, which selects every code area and needs a reviewer.
