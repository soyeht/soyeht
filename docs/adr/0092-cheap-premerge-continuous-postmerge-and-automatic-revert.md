# ADR 0092: A cheap gate before the merge, a continuous gate after it, and an automatic revert

- Status: accepted
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
- desktop: `fmt`, `clippy` (both feature sets) and `cargo test --lib --bins`;
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
`always` area, whose steps run those tests, and still asks for a review. Beside the script
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
`--remote HOST --remote-root PATH`. The steps of the areas `engine`, `desktop` and `e2e` then
run on the host over ssh through `scripts/remote_gate.sh`, which fetches the commit, checks it
out in a persistent worktree of the checkout at PATH and runs
`check_all.sh --tier full --only STEPS` there. The remaining steps (the Swift packages, the
Mac and iOS apps, the editor measurement, the dictation tests, the timing tests, the
`always` checks) run on the local machine, after the remote ones. An unreachable host, a
missing checkout or script, or a commit that cannot be fetched is a problem of the machine:
nothing is recorded and nothing is reverted. The interface is in `docs/dev-local.md`.

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
