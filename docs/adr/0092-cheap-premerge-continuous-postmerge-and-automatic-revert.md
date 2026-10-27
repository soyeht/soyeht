# ADR 0092: A cheap gate before the merge, a continuous gate after it, and an automatic revert

- Status: accepted; the desktop and e2e steps of the fast tier are amended at the end
- Date: 2026-10-16

## Context

ADR 0089 split the gate in two, but its fast tier still ran heavy work before the merge: the
Mac and iOS app builds, the engine and desktop integration tests, the Swift and dictation test
suites. On one shared machine that took 30 to 50 minutes per pull request, and the merges
queued behind it. The aim is the model of a repository that merges many changes a day: no
global lock before the merge, only cheap checks before it, the integration tests after it on
main, an automatic bisect and an automatic revert, and a review only for risky changes.

## Decision

**The fast tier is cheap.** It runs every `always` check and, for the selected areas, format,
lint, a build and the unit tests, in a few minutes on a warm cache:

- engine: `fmt`, `clippy -D warnings`, `cargo build` of the workspace and
  `cargo test --lib --bins` of the changed crates and their reverse dependencies;
- desktop: `fmt`, `clippy` (both feature sets) and `cargo test --lib --bins` (amended below:
  the whole `cargo test`);
- Swift: `swift format lint` and `swift build` of `apps/shared`;
- TypeScript: install, `check`, `test`.

These are `full`-only steps now, run by `scripts/post_merge.py` on main: the Rust integration
tests (`tests/` directories, which start sibling binaries) as `engine:test` and
`desktop:test`, `swift:engine-binaries`, `swift:test-shared`, `swift:test-dictation`, the Mac
and iOS app builds (`check_mac.sh --fast` and `check_ios.sh --fast` are gone), e2e, the editor
measurement and the timing tests. The sibling-binary edges of `gate_plan.py` (`TEST_EDGES`,
the platform and the CLI always tested, the Swift suites launched by engine binaries) went
with them: a unit test starts no other crate. A fast step that builds in place of a full test
is named `<full id>-build`. Moving steps from `both` to `full` lowers the per-area count that
the verification ratchet watches, so the pull request that makes the move carries a
`Verification change:` line; later pull requests are held to the new counts.

**A gate-script change still runs everything, except a test-only one.** A change to the
scripts that run the gate upgrades the run to the full tier (ADR 0089), because a partial run
cannot vouch for them. A change to `scripts/test_*.py` alone no longer does: it selects the
`always` and `gate_scripts` areas, whose steps run those tests, and still asks for a review. Beside the script
it tests, the script decides.

**The gate runs continuously after the merge, and a confirmed culprit is reverted.**
`scripts/main_watch.py` fetches `origin/main`; when it moved since the last pass recorded in
`.cache/main-health/watch.json`, it calls `post_merge.py` once on the newest commit, a batch
of every commit that landed meanwhile. `post_merge.py` is unchanged in kind: it reruns a red
step once (a pass is `flaky` and blames nobody) and otherwise searches the commits since the
last green main. On a confirmed single culprit, `main_watch.py --apply`:

1. creates the branch `main-watch/revert-<sha>` from `origin/main` and reverts the culprit on
   it with `git revert` (against the first parent for a merge commit);
2. pushes the branch as a new branch, never forced;
3. opens a pull request titled "Revert <subject>" whose body names the failing step, the
   culprit and the end of the log;
4. merges it with `gh pr merge --squash`. The revert skips the fast gate: it restores a state
   that was green.

It does not revert, and records and reports instead, when the culprit is ambiguous (several
candidates because some commits cannot run the failing step), when the run was flaky or the
failing step is unknown, when the culprit is itself a revert, when it was reverted already,
when the revert does not apply cleanly, or when the pull request cannot merge (it stays
open). It touches only its own revert branch and never rewrites history. `--dry-run` is the
default; the coordinator runs it with `--apply`. Main is kept green by reverting the
culprit; its author re-lands a fixed change.

**A remote host runs the Linux-capable steps.** `post_merge.py` and `main_watch.py` take
`--remote HOST --remote-root PATH`. The steps of the areas `engine`, `desktop`, `e2e` and `timing` then
run on the host over ssh through `scripts/remote_gate.sh`, which fetches the commit, checks it
out in a persistent worktree of the checkout at PATH and runs
`check_all.sh --tier full --only STEPS` there. The remaining steps (the Swift packages, the
Mac and iOS apps, the editor measurement, the dictation tests, the `always` checks and the
`gate_scripts` unit tests) run on the local machine, after the remote ones. (The timing steps
first ran here; they moved to the host because a latency limit is only meaningful on a quiet
machine, `docs/dev-local.md`.) An unreachable host, a
missing checkout or script, or a commit that cannot be fetched is a problem of the machine:
nothing is recorded and nothing is reverted. The interface is in `docs/dev-local.md`.

**A missing editor measurement is not a red main, and the gate runs in a warm worktree.** The
editor check validates the VM1 report `.cache/editor-performance-vm.json`, which a fresh worktree
does not have. `post_merge.py` and `main_watch.py` take `--editor-meter REPORT` (env
`SOYEHT_EDITOR_METER`) and copy the report into the run's worktree before the gate. When
`editor-performance:check` fails with "missing ..." or "editor paths changed; measure this editor
version again on VM1", the run runs the steps the check hid and, if they pass, is recorded as
`needs-measurement` with the editor tree hash to measure: in the health record and in `watch.json`,
with exit code 4. It starts no culprit search and no revert, and `main_watch.py` leaves the pass
open; the commit runs again once `--editor-meter` names a report carrying that hash. Any other
failure of the check, a latency of 8 ms or more on a matching hash included, stays red. The
second change is `--worktree DIR` (env `SOYEHT_POST_MERGE_WORKTREE`): the run reuses one
persistent worktree instead of a temporary one, with `git checkout --detach --force` and
`git clean -fdx` that keep `engine/target`, `apps/desktop/target`, `.cache/apple-packages` and the
VM1 report, so a Rust build is no longer cold. The Swift `.build` directories and Xcode's derived
data are not kept: their incremental builds miss cross-module changes across checkouts (a default
argument added to generated initializers left stale test objects and an "Undefined symbols" link
error). The culprit search probes in the same worktree. Only a worktree the script created, marked
in its git directory, is reused: the main checkout, the checkout the script runs from, any other
worktree and any other directory are refused, so the force and the clean cannot reach anyone's
work. A relative path is resolved before any check. A worktree that cannot be prepared (a
killed git left `index.lock`) is recreated, and the persistent worktree is never removed as a
stale one. The Rust half of the editor check is the engine step `editor-performance:rust`, run
before the check and, with `--remote`, on the quiet Linux host. A timing-sensitive step (that one
and the `timing` area) records the host's load average, and one that fails on a busy host is rerun
once on the remote host, or after the load drops, before main is called red: the extension of the
flaky rerun above (see docs/dev-local.md). A host that stays busy, or a rerun on the remote host that
passes where the Mac failed, blames no commit: the result is `flaky`. The probes of a latency failure run older commits with the
newest report: a probe that fails as stale or missing is skipped, so such a commit is never
blamed and the revert is not made on a guess.

Known limits, left as follow-ups: the editor hash rule (`editor_path`) comes from the running
script, not from the commit under test, so a commit that changes the rule is judged by the old one;
and the lock is per health directory, so two runs with different `--health-dir` values and the
same `--worktree` are not serialized.

**Review is for risky changes.** `gate_plan.py` keeps `review: required|optional`
(ADR 0089). A pull request whose plan says `optional` merges on a green fast tier with no
separate reviewer; one that says `required` is reviewed by a second agent first. The full
tier after the merge, and the revert, are the safety net for what the fast tier cannot see.

## Consequences

Most pull requests merge in minutes. A change can reach main with a failure that only a
post-merge step finds; it is then reverted by the next pass, usually within one full-tier
run. Until then main is red, the plan prints the warning, and a pull request that touches the
affected area should wait for the revert. A culprit whose revert conflicts with later work, or
that is itself a revert, needs a person.

## Amendment (2026-10-26): the desktop tests and e2e run before the merge

Two regressions reached main because the fast tier could not see them. #622 changed the order
of `file.save` and broke `apps/desktop/crates/soyeht-client/tests/engine.rs`, which only the
full `desktop:test` ran; #635 changed the usage probe and broke `e2e:editor`, which only the
full tier ran. Both were found by the run after the merge and fixed by a second pull request.

The desktop's `tests/` directories drive the client against a scripted engine socket, start no
sibling binary and take a few seconds (`desktop:test` has a median of 34 s, against 47 s for
the stand-in measured on a loaded slot). They are not the heavy integration tests this ADR
moved out. So:

- `desktop:test` runs in both tiers and `desktop:test-fast` is gone;
- the e2e suites run in the fast tier when the engine selection is the whole workspace or its
  reverse closure holds a program they start (`E2E_PROGRAMS` in `gate_plan.py`): the area `e2e`
  is derived from the engine selection, not from a path. About 23 s of suites plus the two
  builds. `engine:build-product` and `engine:build-test-engine` are `e2e` steps placed after
  every `cargo test` of the engine, because `cargo test` relinks `target/debug/soyeht-engine`
  with the test hooks (ADR 0091). The engine integration tests (`engine:test`, 220 to 340 s)
  stay in the full tier. The remote host runs the same steps in the same order
  (`remote_gate.sh` calls `check_all.sh --only`, which runs in file order).

The cost is paid by the unit tests of the gate scripts, which the `always` area ran on every
pull request (about 190 s: `post-merge:unit` 100 s, `quality:unit` 35 s, `main-watch:unit` 29 s,
and the waiver, list and timings tests). They are the area `gate_scripts`, selected by a change
under `scripts/`, `contracts/*.py` or `.githooks/` and by the full tier. `gate-plan:unit` stays
in `always`: it reads engine and desktop sources through `CROSS_READS`, so any change can break
it. Moving them out of `always` lowers the ratchet's per-area count; the pull request carries
the `Verification change:` line.
