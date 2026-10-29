#!/usr/bin/env python3
"""Run the full gate on a main commit and find the culprit when it is red (ADR 0089).

    post_merge.py [--sha COMMIT] [--health-dir DIR] [--keep] [--force]
                  [--remote HOST --remote-root PATH] [--editor-meter REPORT] [--worktree DIR]

In a clean worktree of the commit (default: `origin/main` after a fetch) it runs
`scripts/check_all.sh --tier full` against the commit's parent, then records
`<health-dir>/<sha>.json`, the log `<sha>.log`, `latest.json` and, when green,
`last_green.json`. A `needs-measurement` run (every step passed except the editor measurement,
which VM1 collects separately) counts as green here: it is where the culprit search starts. A
`flaky` or `fail` run never does. The directory is `--health-dir`, else `$SOYEHT_MAIN_HEALTH_DIR`, else
`.cache/main-health` of the main checkout (`gate_base.main_health_dir`, which the fast plan reads too).

When the gate is red it first reruns the failing step once on the same commit: if that
passes the failure is recorded as flaky and no commit is blamed, but the run goes on: the steps the
failed run never reached are run too (the gate stops at its first failure), so `flaky` says that the
rest passed. One of them that fails is judged in its turn (rerun, then culprit search). Every flake is
added to the ledger `<health-dir>/flakes.json` (step, sha, load, date; `gate_flakes.py`), from which
`main_watch.py` reports a step with 2 or more flakes in the last 10 runs as `chronic`. A timing-sensitive step (a
latency limit: the `timing` area and `editor-performance:rust`) records the load average of the
host it ran on, and when that is over `LOAD_PER_CPU_LIMIT` per processor the one rerun is made
after waiting (all the waiting of a run shares `LOAD_WAIT_BUDGET_SECONDS`) for the load to drop;
the culprit search waits the same way before each probe, on the host the rerun used. With `--remote`
that host is the quiet Linux one: the timing steps are in `REMOTE_AREAS`, so they run there from
the first run. A host still busy after the budget makes a failed rerun inconclusive (recorded
`flaky`) and a probe that fails on it is skipped, so it blames nobody. Otherwise it finds the
culprit among the first-parent commits since the last green main, running only the failing
step (`check_all.sh --only STEP_ID`, with the steps it depends on) on a binary search of
them. It never pushes, reverts or edits main: it reports the culprit commit, the failing
step and the log, and exits 1. A commit that cannot run the step, or cannot be checked
out, is skipped; if that leaves several candidates, all of them are reported.

With `--remote HOST --remote-root PATH` the steps of the areas that run on Linux (`REMOTE_AREAS`:
engine, desktop, e2e, timing) run on HOST over ssh, through `scripts/remote_gate.sh` of the checkout at
PATH (docs/dev-local.md), and the other steps (the Swift packages, the Mac and iOS apps, the
editor measurement, the dictation tests, the `always` checks) run here. The
Rust half of the editor check is the engine step `editor-performance:rust`, so it runs on the
host too. The remote steps run first; a failure there stops the run, as a local failure does.

The editor measurement is the VM1 report `.cache/editor-performance-vm.json`, which a fresh
worktree does not have. `--editor-meter REPORT` (env `SOYEHT_EDITOR_METER`) copies it into the
run's worktree before the gate. Without a valid report for the commit's editor files,
`editor-performance:check` fails for a reason that is not main's: the run is recorded as
`needs-measurement` with the editor tree hash to measure, the steps after it still run, nothing is
reverted and no culprit is searched, and the exit code is 4. A measured latency at or over the
limit on a matching hash stays red. A `needs-measurement` commit runs again once `--editor-meter`
names a report for its hash.

`--worktree DIR` (env `SOYEHT_POST_MERGE_WORKTREE`) keeps one worktree between runs instead of a
temporary one, so the cargo targets stay warm: each run checks out the commit there with
`git checkout --detach --force` and `git clean -fdx`, keeping `engine/target`,
`apps/desktop/target`, `.cache/apple-packages` and the VM1 report. The Swift `.build` directories
and Xcode's derived data go: their incremental builds miss cross-module changes across checkouts
and leave stale objects. The culprit search checks out its probes there too. Only a worktree this
script created (it marks its git directory) is ever reused; the main checkout, the checkout the
script runs from, any other worktree and any other directory are refused, and a worktree that
cannot be prepared (a killed git left `index.lock`) is recreated. DIR is for this script alone:
never point another worktree's build at the same directories.

A `--force` rerun of a recorded commit keeps the run it replaces: that run's log, timings and
record move to `<health-dir>/history/<sha>.<started_at>.*`, and the new record names it in
`previous` (its result, failed step and log), so a flaky failure stays evidence. Nothing prunes
`history/`, and an archive is never overwritten (a repeated stem gets a `-N` suffix).

Only one run at a time (a lock in the health directory). A problem of the machine, not of
main (no restricted vocabulary, a gate that cannot start) writes no record and exits 2.
Every gate run gets `--event-file` with the checked commit's message as a pull request (subject
as title, the rest as body), because a squash merge keeps the pull request's text there: the
`Verification change:` waiver of `hardening_gate.py` and the forbidden-terms scan read it as
they read the pull request before the merge. The culprit search checks each commit with its own.

The coordinator session runs this after each merge or every few merges; the editor
measurement on the VM is collected separately, when the editor hash changes.
"""

from __future__ import annotations

import argparse
import fcntl
import json
import os
import shutil
import signal
import sys
import tempfile
import time
from pathlib import Path

import gate_flakes
import post_merge_hosts as hosts
from gate_base import main_checkout, main_health_dir
from gate_worktree import (WORKTREE_PREFIX, InfrastructureError, git, prepare_persistent_worktree,
                           remove_stale_worktrees, scratch_root)
from post_merge_editor import (EDITOR_METER_ENV, EDITOR_STEP, GREEN_RESULTS, NEEDS_MEASUREMENT,
                               NEEDS_MEASUREMENT_EXIT, install_editor_meter, lacks_measurement, measured_since)
from post_merge_gate import INFRASTRUCTURE_EXITS, failing_step, run_gate
from post_merge_hosts import Remote
from post_merge_investigate import investigate, settle_missing_measurement
from post_merge_records import archive_previous_run, commit_info, parent_of, read_record, report, write_json

BUSY_EXIT = 75
# A persistent worktree (--worktree), see gate_worktree.py, keeps its cargo targets between runs.
WORKTREE_ENV = "SOYEHT_POST_MERGE_WORKTREE"


def run(root: Path, sha: str, health: Path, command: list[str], since: str | None = None,
        keep: bool = False, remote: Remote | None = None, editor_meter: Path | None = None,
        worktree_dir: Path | None = None) -> dict:
    sha = git(root, "rev-parse", sha)
    log, timings = health / f"{sha}.log", health / f"{sha}.timings.json"
    health.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix=WORKTREE_PREFIX, dir=scratch_root()))
    worktree = worktree_dir or scratch / "tree"
    try:
        if worktree_dir:
            prepare_persistent_worktree(root, worktree, sha)
        else:
            git(root, "worktree", "add", "--detach", str(worktree), sha)
        meter = install_editor_meter(editor_meter, worktree)
        started = time.time()
        previous = archive_previous_run(health, sha)
        log.write_bytes(b"")  # the earlier run, if any, is in the history now
        code = run_gate(worktree, parent_of(root, sha), None, log, timings, command, remote)
        if code in INFRASTRUCTURE_EXITS:
            raise InfrastructureError(f"the gate could not run (exit {code}); see {log}")
        record: dict = {
            "sha": sha,
            "subject": commit_info(root, sha)["subject"],
            "result": "pass" if code == 0 else "fail",
            "exit_code": code,
            "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(started)),
            "duration_seconds": round(time.time() - started, 1),
            "log": str(log),
            "editor_meter": meter,
            "steps": [],
        }
        if previous:
            record["previous"] = previous
        if remote:
            record["remote"] = remote.host
        local = hosts.local_load()  # a run that passed on a loaded host is worth knowing about too
        if local:
            record["local_load"] = local.as_record()
        try:
            record["steps"] = json.loads(timings.read_text())["latest"]["steps"]
        except (OSError, ValueError, KeyError):
            record["note"] = "no per-step timings were recorded"
        if code:
            record["failed_step"] = failing_step(log, timings)
            if record["failed_step"] == EDITOR_STEP and lacks_measurement(log):
                code = settle_missing_measurement(worktree, parent_of(root, sha), record, log,
                                                  scratch, command, remote)
                record["duration_seconds"] = round(time.time() - started, 1)
        if code:
            try:
                investigate(root, worktree, scratch, health, record, since, command, remote,
                            persistent=worktree_dir is not None)
            except RuntimeError as error:  # the record of the red run is still written
                record["note"] = f"the culprit search failed: {error}"
        write_json(health / f"{sha}.json", record)
        write_json(health / "latest.json", record)
        if record["result"] in GREEN_RESULTS:
            write_json(health / "last_green.json", {"sha": sha, "at": record["started_at"]})
        gate_flakes.record_run(health, record)  # last: the ledger never holds back the green marker
        return record
    finally:
        if not worktree_dir and not keep:
            git(root, "worktree", "remove", "--force", str(worktree), check=False)
        if not keep:
            shutil.rmtree(scratch, ignore_errors=True)


def terminate(signal_number: int, frame) -> None:
    raise SystemExit(128 + signal_number)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sha", help="commit to check (default: origin/main after a fetch)")
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--health-dir", type=Path)
    parser.add_argument("--since", help="last known green commit when none is recorded")
    parser.add_argument("--check-all", default="bash scripts/check_all.sh",
                        help="the gate command run in each worktree")
    parser.add_argument("--remote", metavar="HOST",
                        help="run the engine, desktop, e2e and timing steps on this host over ssh")
    parser.add_argument("--remote-root", metavar="PATH",
                        help="the checkout on the host that has scripts/remote_gate.sh")
    parser.add_argument("--editor-meter", type=Path, metavar="REPORT",
                        default=os.environ.get(EDITOR_METER_ENV) or None,
                        help=f"the VM1 editor measurement to copy into the worktree (${EDITOR_METER_ENV})")
    parser.add_argument("--worktree", type=Path, metavar="DIR", default=os.environ.get(WORKTREE_ENV) or None,
                        help=f"reuse this worktree between runs, keeping its build directories (${WORKTREE_ENV})")
    parser.add_argument("--keep", action="store_true", help="keep the temporary worktree")
    parser.add_argument("--force", action="store_true", help="run again for a recorded commit")
    arguments = parser.parse_args(argv)
    if bool(arguments.remote) != bool(arguments.remote_root):
        parser.error("--remote and --remote-root go together")
    remote = Remote(arguments.remote, arguments.remote_root) if arguments.remote else None
    if not os.environ.get("FORBIDDEN_TERMS"):
        print("post_merge: provide FORBIDDEN_TERMS through the environment; nothing was recorded",
              file=sys.stderr)
        return 2
    root = main_checkout(arguments.repo)
    health = arguments.health_dir or main_health_dir(arguments.repo)
    health.mkdir(parents=True, exist_ok=True)
    # Absolute, whatever the working directory: git resolves a relative path against its own.
    worktree_dir = arguments.worktree.resolve() if arguments.worktree else None
    with (health / "post_merge.lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            print("post_merge: another run is in progress", file=sys.stderr)
            return BUSY_EXIT
        signal.signal(signal.SIGTERM, terminate)
        remove_stale_worktrees(root, worktree_dir)
        if arguments.sha is None:
            git(root, "fetch", "origin", "main")
        sha = git(root, "rev-parse", arguments.sha or "origin/main")
        recorded = read_record(health / f"{sha}.json")
        if (health / f"{sha}.json").exists() and not arguments.force and not measured_since(
                recorded, arguments.editor_meter):
            print(f"post_merge: {sha[:12]} is already recorded in {health}")
            return NEEDS_MEASUREMENT_EXIT if recorded.get("result") == NEEDS_MEASUREMENT else 0
        try:
            record = run(root, sha, health, arguments.check_all.split(), arguments.since, arguments.keep,
                         remote, arguments.editor_meter, worktree_dir)
        except InfrastructureError as error:
            print(f"post_merge: {error}; nothing was recorded", file=sys.stderr)
            return 2
    print(report(record))
    if record["result"] == NEEDS_MEASUREMENT:
        return NEEDS_MEASUREMENT_EXIT
    return 0 if record["result"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
