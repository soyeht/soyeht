#!/usr/bin/env python3
"""Run the full gate on a main commit and find the culprit when it is red (ADR 0089).

    post_merge.py [--sha COMMIT] [--health-dir DIR] [--keep] [--force]

In a clean worktree of the commit (default: `origin/main` after a fetch) it runs
`scripts/check_all.sh --tier full` against the commit's parent, then records
`<health-dir>/<sha>.json`, the log `<sha>.log`, `latest.json` and, when green,
`last_green.json`. The default directory is `.cache/main-health` of the main checkout.

When the gate is red it first reruns the failing step once on the same commit: if that
passes the failure is recorded as flaky and no commit is blamed. Otherwise it finds the
culprit among the first-parent commits since the last green main, running only the failing
step (`check_all.sh --only STEP_ID`, with the steps it depends on) on a binary search of
them. It never pushes, reverts or edits main: it reports the culprit commit, the failing
step and the log, and exits 1. A commit that cannot run the step, or cannot be checked
out, is skipped; if that leaves several candidates, all of them are reported.

Only one run at a time (a lock in the health directory). A problem of the machine, not of
main (no restricted vocabulary, a gate that cannot start) writes no record and exits 2.
The coordinator session runs this after each merge or every few merges; the editor
measurement on the VM is collected separately, when the editor hash changes.
"""

from __future__ import annotations

import argparse
import fcntl
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

from gate_base import main_checkout, main_health_dir

GOOD, BAD, SKIP = "good", "bad", "skip"
FAILED_STEP = re.compile(r"^check_all: FAILED step (\S+)", re.MULTILINE)
# check_all.sh exit codes that mean the gate could not run, not that main is red.
UNSUPPORTED_STEP_EXIT = 78
INFRASTRUCTURE_EXITS = {2, 64, 70, 71, UNSUPPORTED_STEP_EXIT}
# A step that only means something after another step of the same run.
PREREQUISITES = {"e2e:tcc-check": "e2e:tcc-snapshot"}
WORKTREE_PREFIX = "soyeht-post-merge-"
BUSY_EXIT = 75


class InfrastructureError(RuntimeError):
    """The machine or the gate could not run; nothing is recorded about main."""


@dataclass
class Search:
    culprit: str | None
    candidates: list[str] = field(default_factory=list)
    probes: dict[str, str] = field(default_factory=dict)


def find_culprit(commits: list[str], probe: Callable[[str], str]) -> Search:
    """`commits` runs oldest to newest; the one before it is good and the last one is bad.

    Returns the first bad commit, or, when skipped commits hide it, the candidates.
    """
    probes: dict[str, str] = {}
    low, high = -1, len(commits) - 1  # commits[low] is good, commits[high] is bad
    skipped: set[int] = set()
    while high - low > 1:
        open_indexes = [i for i in range(low + 1, high) if i not in skipped]
        if not open_indexes:
            break
        middle = min(open_indexes, key=lambda i: abs(i - (low + high) / 2))
        result = probes[commits[middle]] = probe(commits[middle])
        if result == GOOD:
            low = middle
        elif result == BAD:
            high = middle
        else:
            skipped.add(middle)
    if high - low == 1:
        return Search(commits[high], [commits[high]], probes)
    return Search(None, commits[low + 1 : high + 1], probes)


def git(root: Path, *arguments: str, check: bool = True) -> str:
    result = subprocess.run(["git", *arguments], cwd=root, capture_output=True, text=True)
    if check and result.returncode:
        raise RuntimeError(f"git {' '.join(arguments)}: {result.stderr.strip()}")
    return result.stdout.strip()


def run_process(arguments: list[str], cwd: Path, env: dict, output) -> int:
    """Run in its own process group, so a signal to this script stops the whole gate."""
    process = subprocess.Popen(arguments, cwd=cwd, env=env, stdout=output,
                               stderr=subprocess.STDOUT if output != subprocess.DEVNULL else subprocess.DEVNULL,
                               start_new_session=True)
    try:
        return process.wait()
    except BaseException:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait()
        raise


def run_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
             timings: Path, command: list[str]) -> int:
    arguments = [*command, "--tier", "full"]
    if only:
        arguments += ["--only", only]
    if base:
        arguments += ["--base", base]
    environment = {**os.environ, "SOYEHT_GATE_TIMINGS": str(timings)}
    if log is None:
        return run_process(arguments, worktree, environment, subprocess.DEVNULL)
    with log.open("wb") as handle:
        return run_process(arguments, worktree, environment, handle)


def step_with_prerequisites(step: str) -> str:
    prerequisite = PREREQUISITES.get(step)
    return f"{prerequisite},{step}" if prerequisite else step


def failing_step(log: Path, timings: Path) -> str | None:
    try:
        steps = json.loads(timings.read_text())["latest"]["steps"]
        failed = [step["id"] for step in steps if step["status"]]
        if failed:
            return failed[0]
    except (OSError, ValueError, KeyError):
        pass  # fall back to the log line below
    found = FAILED_STEP.findall(log.read_text(errors="replace")) if log.exists() else []
    return found[0] if found else None


def commit_info(root: Path, sha: str) -> dict:
    subject, author = git(root, "log", "-1", "--format=%s%x00%an", sha).split("\0")
    return {"sha": sha, "subject": subject, "author": author}


def parent_of(root: Path, sha: str) -> str | None:
    parents = git(root, "rev-list", "--parents", "-n", "1", sha).split()
    return parents[1] if len(parents) > 1 else None


def supports_only(root: Path, sha: str) -> bool:
    result = subprocess.run(["git", "show", f"{sha}:scripts/check_all.sh"], cwd=root,
                            capture_output=True, text=True)
    return result.returncode == 0 and "--only" in result.stdout


def write_json(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")


def search_range(root: Path, health: Path, sha: str, since: str | None) -> list[str]:
    last_green = since
    green_file = health / "last_green.json"
    if last_green is None and green_file.exists():
        last_green = json.loads(green_file.read_text())["sha"]
    if last_green is None:
        return []
    commits = git(root, "rev-list", "--first-parent", "--reverse", f"{last_green}..{sha}", check=False)
    return commits.split()


def locate_culprit(root: Path, worktree: Path, commits: list[str], step: str,
                   command: list[str], scratch: Path) -> Search:
    selection = step_with_prerequisites(step)

    def probe(commit: str) -> str:
        checkout = subprocess.run(["git", "checkout", "--quiet", "--detach", commit],
                                  cwd=worktree, capture_output=True, text=True)
        if checkout.returncode:
            print(f"post_merge: cannot check out {commit[:12]}: {checkout.stderr.strip()}", flush=True)
            return SKIP
        if not supports_only(root, commit):
            return SKIP
        code = run_gate(worktree, parent_of(root, commit), selection, None, scratch / "probe.json", command)
        print(f"post_merge: {commit[:12]} step {selection}: exit {code}", flush=True)
        if code in INFRASTRUCTURE_EXITS:
            return SKIP
        return GOOD if code == 0 else BAD

    return find_culprit(commits, probe)


def report(record: dict) -> str:
    if record["result"] == "pass":
        return f"post_merge: main {record['sha'][:12]} is green ({record['duration_seconds']:.0f}s)"
    state = "FLAKY (not green)" if record["result"] == "flaky" else "RED"
    lines = [f"post_merge: main {record['sha'][:12]} is {state} (step {record['failed_step']})"]
    culprit = record.get("culprit")
    if culprit:
        lines.append(f"  culprit: {culprit['sha'][:12]} {culprit['subject']} ({culprit['author']})")
    for candidate in record.get("candidates", []):
        if not culprit:
            lines.append(f"  candidate: {candidate['sha'][:12]} {candidate['subject']}")
    if record.get("note"):
        lines.append(f"  note: {record['note']}")
    lines.append(f"  log: {record['log']}")
    lines.append("  Fix forward or revert the culprit within the hour; main is not touched.")
    return "\n".join(lines)


def investigate(root: Path, worktree: Path, scratch: Path, health: Path, record: dict,
                since: str | None, command: list[str]) -> None:
    """Fill in the culprit of a red run: a flaky step blames nobody."""
    sha, step = record["sha"], record["failed_step"]
    if not step:
        record["note"] = "the failing step is unknown, so the culprit was not searched"
        return
    again = run_gate(worktree, parent_of(root, sha), step_with_prerequisites(step), None,
                     scratch / "rerun.json", command)
    if again == 0:
        record["result"] = "flaky"
        record["note"] = (f"step {step} passed when run again on the same commit; no commit is blamed, "
                          "and the steps after it did not run")
        return
    commits = search_range(root, health, sha, since)
    if not commits:
        record["note"] = "no known green main to search from; pass --since"
    elif len(commits) == 1:
        record["culprit"] = commit_info(root, commits[0])
    else:
        result = locate_culprit(root, worktree, commits, step, command, scratch)
        if result.culprit:
            record["culprit"] = commit_info(root, result.culprit)
        else:
            record["candidates"] = [commit_info(root, c) for c in result.candidates]
            record["note"] = "some commits cannot run the failing step alone"


def remove_stale_worktrees(root: Path) -> None:
    """Leftovers of a run that was killed; the lock says no other run is alive."""
    git(root, "worktree", "prune", check=False)
    for stale in Path(tempfile.gettempdir()).glob(f"{WORKTREE_PREFIX}*"):
        shutil.rmtree(stale, ignore_errors=True)
    git(root, "worktree", "prune", check=False)


def run(root: Path, sha: str, health: Path, command: list[str], since: str | None = None,
        keep: bool = False) -> dict:
    sha = git(root, "rev-parse", sha)
    log, timings = health / f"{sha}.log", health / f"{sha}.timings.json"
    health.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix=WORKTREE_PREFIX))
    worktree = scratch / "tree"
    try:
        git(root, "worktree", "add", "--detach", str(worktree), sha)
        started = time.time()
        code = run_gate(worktree, parent_of(root, sha), None, log, timings, command)
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
            "steps": [],
        }
        try:
            record["steps"] = json.loads(timings.read_text())["latest"]["steps"]
        except (OSError, ValueError, KeyError):
            record["note"] = "no per-step timings were recorded"
        if code:
            record["failed_step"] = failing_step(log, timings)
            try:
                investigate(root, worktree, scratch, health, record, since, command)
            except RuntimeError as error:  # the record of the red run is still written
                record["note"] = f"the culprit search failed: {error}"
        write_json(health / f"{sha}.json", record)
        write_json(health / "latest.json", record)
        if not code:
            write_json(health / "last_green.json", {"sha": sha, "at": record["started_at"]})
        return record
    finally:
        if not keep:
            git(root, "worktree", "remove", "--force", str(worktree), check=False)
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
    parser.add_argument("--keep", action="store_true", help="keep the worktree")
    parser.add_argument("--force", action="store_true", help="run again for a recorded commit")
    arguments = parser.parse_args(argv)
    if not os.environ.get("FORBIDDEN_TERMS"):
        print("post_merge: provide FORBIDDEN_TERMS through the environment; nothing was recorded",
              file=sys.stderr)
        return 2
    root = main_checkout(arguments.repo)
    health = arguments.health_dir or main_health_dir(arguments.repo)
    health.mkdir(parents=True, exist_ok=True)
    with (health / "post_merge.lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            print("post_merge: another run is in progress", file=sys.stderr)
            return BUSY_EXIT
        signal.signal(signal.SIGTERM, terminate)
        remove_stale_worktrees(root)
        if arguments.sha is None:
            git(root, "fetch", "origin", "main")
        sha = git(root, "rev-parse", arguments.sha or "origin/main")
        if (health / f"{sha}.json").exists() and not arguments.force:
            print(f"post_merge: {sha[:12]} is already recorded in {health}")
            return 0
        try:
            record = run(root, sha, health, arguments.check_all.split(), arguments.since, arguments.keep)
        except InfrastructureError as error:
            print(f"post_merge: {error}; nothing was recorded", file=sys.stderr)
            return 2
    print(report(record))
    return 0 if record["result"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
