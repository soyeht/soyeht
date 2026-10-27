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
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

import check_editor_performance
import gate_flakes
from gate_base import main_checkout, main_health_dir
from gate_worktree import (EDITOR_REPORT, WORKTREE_PREFIX, InfrastructureError, clean_worktree, git,
                           prepare_persistent_worktree, remove_stale_worktrees, scratch_root)

GOOD, BAD, SKIP = "good", "bad", "skip"
FAILED_STEP = re.compile(r"^check_all: FAILED step (\S+)", re.MULTILINE)
# check_all.sh exit codes that mean the gate could not run, not that main is red.
UNSUPPORTED_STEP_EXIT = 78
INFRASTRUCTURE_EXIT = 2
INFRASTRUCTURE_EXITS = {INFRASTRUCTURE_EXIT, 64, 70, 71, UNSUPPORTED_STEP_EXIT}
# A step that only means something after another step of the same run.
PREREQUISITES = {"e2e:tcc-check": "e2e:tcc-snapshot"}
HISTORY_DIRECTORY = "history"
BUSY_EXIT = 75
# The editor measurement (ADR 0092): the VM1 report is copied into the worktree, and a check that
# fails only because it is missing or stale is not a red main.
EDITOR_METER_ENV = "SOYEHT_EDITOR_METER"
EDITOR_STEP = "editor-performance:check"
NEEDS_MEASUREMENT = "needs-measurement"
# Results from which the culprit search may start: no step failed. `needs-measurement` is only
# set when every step passed except the editor measurement, which is collected apart on VM1.
GREEN_RESULTS = ("pass", NEEDS_MEASUREMENT)
NEEDS_MEASUREMENT_EXIT = 4
# What check_editor_performance.py prints for an absent report and for one of other editor files.
MISSING_REPORT = "editor-performance: missing "
STALE_REPORT = "editor paths changed; measure this editor version again on VM1"
# A persistent worktree (--worktree), see gate_worktree.py, keeps its cargo targets between runs.
WORKTREE_ENV = "SOYEHT_POST_MERGE_WORKTREE"
# The Rust half of the editor check: an `engine` step (so it runs on the remote host) with a latency
# limit, therefore timing-sensitive.
EDITOR_RUST_STEP = "editor-performance:rust"
# Timing-sensitive steps: a latency limit that a loaded host trips without any regression. Every
# step of the `timing` area is one, and so are these.
TIMING_STEPS = (EDITOR_RUST_STEP,)
TIMING_AREA = "timing"
# A host is busy when its 1-minute load average per processor is over this (env LOAD_LIMIT_ENV): the
# Mac measured 243 ms for a 150 ms limit at a load of 60 to 140, and 92 ms on a quiet host.
LOAD_PER_CPU_LIMIT = 1.0
LOAD_LIMIT_ENV = "SOYEHT_LOAD_PER_CPU_LIMIT"
# The longest a whole run waits for busy hosts to quiet down, the rerun and every probe of the
# culprit search together (WaitBudget); after it a busy host is inconclusive, not a regression.
LOAD_WAIT_BUDGET_SECONDS = 1800
LOAD_POLL_SECONDS = 30
LOAD_PROBE_TIMEOUT_SECONDS = 30  # the ssh that reads a host's load
# The areas whose steps run on the remote Linux host; every other step runs on this machine. The
# `timing` area is one of them: a latency limit is only meaningful on a quiet machine, the host
# is the one that is, and its tests are the engine's own (the same crate the host builds warm for
# `engine:test`). On the Mac they ran at three to four times the load limit and flaked.
REMOTE_AREAS = ("engine", "desktop", "e2e", TIMING_AREA)
# Set to a command (split on example) to replace `ssh -o BatchMode=yes`; the tests use it.
SSH_ENV = "SOYEHT_REMOTE_SSH"
REMOTE_UNSUPPORTED_EXIT = UNSUPPORTED_STEP_EXIT
SSH_FAILED_EXIT = 255  # ssh itself could not connect or run the command
# scripts/remote_gate.sh: the host's worktree is held by another run. Not BUSY_EXIT, which is
# this machine's own lock; neither says anything about the commit.
REMOTE_BUSY_EXIT = 75


@dataclass(frozen=True)
class Remote:
    host: str
    root: str  # a checkout of the repository on the host that has scripts/remote_gate.sh

    def remote_path(self) -> str:
        """The root as the host's shell should read it: a leading `~` is the host's home."""
        if self.root == "~" or self.root.startswith("~/"):
            return '"$HOME"' + shlex.quote(self.root[1:])
        return shlex.quote(self.root)

    def ssh(self, command: str) -> list[str]:
        ssh = os.environ.get(SSH_ENV, "ssh -o BatchMode=yes").split()
        # A login shell, so that the host's toolchain is on PATH. A missing checkout or script is
        # a problem of the host (exit 2), not a red main.
        script = (f"cd {self.remote_path()} || exit 2; "
                  f"[ -f scripts/remote_gate.sh ] || exit 2; {command}")
        return [*ssh, self.host, "bash -lc " + shlex.quote(script)]


@dataclass(frozen=True)
class Load:
    """The 1-minute load average of a host and its processor count."""

    host: str
    average: float
    cpus: int

    @property
    def per_cpu(self) -> float:
        return self.average / max(self.cpus, 1)

    def busy(self) -> bool:
        return self.per_cpu > float(os.environ.get(LOAD_LIMIT_ENV) or LOAD_PER_CPU_LIMIT)

    def describe(self) -> str:
        return f"load average {self.average:.1f} on {self.cpus} cpus of {self.host}"

    def as_record(self) -> dict:
        return {"host": self.host, "average": round(self.average, 2), "cpus": self.cpus}


def local_load() -> Load | None:
    try:
        return Load("local", os.getloadavg()[0], os.cpu_count() or 1)
    except OSError:
        return None


def remote_load(remote: Remote) -> Load | None:
    """The host's load, or nothing when it cannot say (a busy, unreachable or silent host)."""
    try:
        result = subprocess.run(remote.ssh("bash scripts/remote_gate.sh --print-load"),
                                capture_output=True, text=True, timeout=LOAD_PROBE_TIMEOUT_SECONDS)
        average, cpus = result.stdout.split()
        return Load(remote.host, float(average), int(cpus)) if result.returncode == 0 else None
    except (subprocess.TimeoutExpired, OSError, ValueError):
        return None


def pause(seconds: float) -> None:
    time.sleep(seconds)


@dataclass
class WaitBudget:
    """The seconds a run may still spend waiting for busy hosts: one budget for the rerun and all
    the probes, so that a machine that never quiets down costs a bounded time."""

    left: float = LOAD_WAIT_BUDGET_SECONDS


def wait_until_quiet(measure: Callable[[], Load | None], budget: WaitBudget) -> Load | None:
    """Wait, within the budget, until `measure()` says the host is not busy; the last load."""
    load = measure()
    while load is not None and load.busy() and budget.left > 0:
        print(f"post_merge: waiting for a quiet host ({load.describe()})", flush=True)
        seconds = min(LOAD_POLL_SECONDS, budget.left)
        pause(seconds)
        budget.left -= seconds
        load = measure()
    return load


def busy_after(measure: Callable[[], Load | None]) -> Load | None:
    """The host's load when it is busy now, else nothing."""
    load = measure()
    return load if load is not None and load.busy() else None


def is_timing_sensitive(step: str, areas: dict[str, str] | None) -> bool:
    return step in TIMING_STEPS or (areas or {}).get(step) == TIMING_AREA


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


def commit_event(worktree: Path) -> dict:
    """The pull request event of the commit checked out in `worktree`. A squash merge makes the
    pull request's title the subject and its body the rest of the message, so the checks that
    read the event (the `Verification change:` waiver of `hardening_gate.py`, the forbidden
    terms) see the same text after the merge as before it."""
    subject, _, body = git(worktree, "log", "-1", "--format=%B", "HEAD").partition("\n")
    return {"pull_request": {"title": subject, "body": body.strip()}}


def run_local_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
                   timings: Path, command: list[str]) -> int:
    arguments = [*command, "--tier", "full"]
    if only:
        arguments += ["--only", only]
    if base:
        arguments += ["--base", base]
    environment = {**os.environ, "SOYEHT_GATE_TIMINGS": str(timings)}
    # Outside the worktree: the checks scan its untracked files. The event goes to every step
    # (check_all.sh passes its arguments on), as in a pull request run.
    with tempfile.TemporaryDirectory(dir=scratch_root()) as directory:
        event = Path(directory) / "event.json"
        write_json(event, commit_event(worktree))
        arguments += ["--event-file", str(event)]
        if log is None:
            return run_process(arguments, worktree, environment, subprocess.DEVNULL)
        with log.open("ab") as handle:
            return run_process(arguments, worktree, environment, handle)


def gate_step_areas(worktree: Path, command: list[str]) -> dict[str, str] | None:
    """{step id: area} of the full tier, or None when the commit's gate cannot list its steps."""
    result = subprocess.run([*command, "--tier", "full", "--list"], cwd=worktree,
                            capture_output=True, text=True)
    if result.returncode:
        return None
    rows = [line.split() for line in result.stdout.splitlines()]
    return {row[2]: row[0] for row in rows if len(row) == 3}


def remote_steps_timings(remote: Remote) -> list[dict]:
    """The per-step times of the last remote run, or nothing when the host cannot say."""
    result = subprocess.run(remote.ssh("bash scripts/remote_gate.sh --print-timings"),
                            capture_output=True, text=True)
    try:
        return json.loads(result.stdout)["latest"]["steps"] if result.returncode == 0 else []
    except (ValueError, KeyError, TypeError):
        return []


def run_split_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
                   timings: Path, command: list[str], remote: Remote) -> int:
    """The remote areas run on the host, then the rest here; the first failure ends the run."""
    areas = gate_step_areas(worktree, command)
    if areas is None:
        return REMOTE_UNSUPPORTED_EXIT
    wanted = only.split(",") if only else list(areas)
    remote_ids = [i for i in areas if i in wanted and areas[i] in REMOTE_AREAS]
    local_ids = [i for i in areas if i in wanted and areas[i] not in REMOTE_AREAS]
    local_ids += [i for i in wanted if i not in areas]  # the local gate reports an unknown step
    sha = git(worktree, "rev-parse", "HEAD")
    timings.unlink(missing_ok=True)
    steps: list[dict] = []
    code = 0
    if remote_ids:
        arguments = f"bash scripts/remote_gate.sh --sha {sha} --steps {','.join(remote_ids)}"
        if base:
            arguments += f" --base {base}"
        code = run_process_logged(remote.ssh(arguments), worktree, log)
        if code in (SSH_FAILED_EXIT, REMOTE_BUSY_EXIT):
            code = INFRASTRUCTURE_EXIT
        steps += remote_steps_timings(remote)
    if not code and local_ids:
        local_timings = timings.with_name(timings.name + ".local")
        local_timings.unlink(missing_ok=True)
        code = run_local_gate(worktree, base, ",".join(local_ids), log, local_timings, command)
        try:
            steps += json.loads(local_timings.read_text())["latest"]["steps"]
        except (OSError, ValueError, KeyError):
            pass  # a gate that failed to start has no timings; the exit code says so
    if steps:
        write_json(timings, {"latest": {"steps": steps}})
    return code


def run_process_logged(arguments: list[str], cwd: Path, log: Path | None) -> int:
    if log is None:
        return run_process(arguments, cwd, os.environ.copy(), subprocess.DEVNULL)
    with log.open("ab") as handle:
        return run_process(arguments, cwd, os.environ.copy(), handle)


def run_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
             timings: Path, command: list[str], remote: Remote | None = None) -> int:
    if remote is None:
        return run_local_gate(worktree, base, only, log, timings, command)
    return run_split_gate(worktree, base, only, log, timings, command, remote)


def step_with_prerequisites(step: str) -> str:
    prerequisite = PREREQUISITES.get(step)
    return f"{prerequisite},{step}" if prerequisite else step


def failing_step(log: Path, timings: Path, last: bool = False) -> str | None:
    try:
        steps = json.loads(timings.read_text())["latest"]["steps"]
        failed = [step["id"] for step in steps if step["status"]]
        if failed:
            return failed[0]
    except (OSError, ValueError, KeyError):
        pass  # fall back to the log line below
    found = FAILED_STEP.findall(log.read_text(errors="replace")) if log.exists() else []
    return found[-1 if last else 0] if found else None


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


def archive_previous_run(health: Path, sha: str) -> dict | None:
    """Keep the run a `--force` rerun replaces, and say what it was.

    The first, failing, run is the only evidence of a flake. Its log and timings move to
    `<health>/history/<sha>.<started_at>.*`, and its record is copied there (the record stays
    in place until the new one replaces it, so a rerun that cannot start loses nothing).
    Returns a summary for the `previous` field of the new record, or None when nothing was run."""
    names = {"log": f"{sha}.log", "json": f"{sha}.json", "timings.json": f"{sha}.timings.json"}
    sources = [health / name for name in names.values() if (health / name).exists()]
    if not sources:
        return None
    previous = read_record(health / names["json"])
    started = previous.get("started_at") or time.strftime(
        "%Y-%m-%dT%H:%M:%SZ", time.gmtime(max(path.stat().st_mtime for path in sources)))
    history = health / HISTORY_DIRECTORY
    history.mkdir(parents=True, exist_ok=True)
    # A rerun that died before it wrote its record leaves this `started_at` in place: the next
    # archive must not replace the evidence of the first one.
    base_stem = f"{sha}.{started.replace(':', '')}"
    stem, attempt = base_stem, 1
    while any((history / f"{stem}.{suffix}").exists() for suffix in names):
        stem, attempt = f"{base_stem}-{attempt}", attempt + 1
    for suffix, name in names.items():
        source = health / name
        if not source.exists():
            continue
        if suffix == "json":
            shutil.copyfile(source, history / f"{stem}.{suffix}")
        else:
            source.replace(history / f"{stem}.{suffix}")
    summary = {key: previous[key] for key in ("result", "exit_code", "failed_step", "culprit")
               if key in previous}
    return {**summary, "started_at": started, "log": str(history / f"{stem}.log")}


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
                   command: list[str], scratch: Path, remote: Remote | None = None,
                   persistent: bool = False, measure: Callable[[], Load | None] | None = None,
                   budget: WaitBudget | None = None) -> Search:
    """`measure` (a timing step) reads the load of the host the probes run on: a probe waits for it
    to be quiet, and one that fails on a host that is (or turned) busy is skipped, so a busy host
    never blames a commit; one that passes is good wherever it ran."""
    selection = step_with_prerequisites(step)
    # The probes of the editor check run older commits with the report of the newest one: a commit
    # whose editor files differ fails as stale, which says nothing about its latency.
    probe_log = scratch / "probe.log" if step == EDITOR_STEP else None

    def probe(commit: str) -> str:
        checkout = subprocess.run(
            ["git", "checkout", "--quiet", "--detach", *(["--force"] if persistent else []), commit],
            cwd=worktree, capture_output=True, text=True)
        if persistent and not checkout.returncode:
            clean_worktree(worktree)
        if checkout.returncode:
            print(f"post_merge: cannot check out {commit[:12]}: {checkout.stderr.strip()}", flush=True)
            return SKIP
        if not supports_only(root, commit):
            return SKIP
        if probe_log:
            probe_log.unlink(missing_ok=True)
        waited_busy = False
        if measure:  # a timing step waits for a quiet host, but is probed on a busy one too
            load = wait_until_quiet(measure, budget or WaitBudget())
            waited_busy = load is not None and load.busy()
        code = run_gate(worktree, parent_of(root, commit), selection, probe_log, scratch / "probe.json",
                        command, remote)
        print(f"post_merge: {commit[:12]} step {selection}: exit {code}", flush=True)
        if code in INFRASTRUCTURE_EXITS or (code and probe_log and lacks_measurement(probe_log)):
            return SKIP
        # Load only slows a step: a pass is sound evidence on any host, a failure on a busy one is not.
        if code and measure and (waited_busy or busy_after(measure) is not None):
            return SKIP
        return GOOD if code == 0 else BAD

    return find_culprit(commits, probe)


def report(record: dict) -> str:
    if record["result"] == "pass":
        return f"post_merge: main {record['sha'][:12]} is green ({record['duration_seconds']:.0f}s)"
    if record["result"] == NEEDS_MEASUREMENT:
        return (f"post_merge: main {record['sha'][:12]} needs a VM1 editor measurement "
                f"(editor tree {record['editor_tree_hash']}); main is not red\n  note: {record['note']}\n"
                f"  log: {record['log']}")
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


@dataclass
class Rerun:
    """The one rerun of a failing step: its exit code, how it ran, and where it ran."""

    code: int
    measure: Callable[[], Load | None]  # the load of the host the step runs on
    note: str = ""
    busy_after: Load | None = None  # the host was busy when a failed rerun ended: inconclusive
    load: Load | None = None  # the load of that host before the rerun, for a timing step


def rerun_on_quiet_host(worktree: Path, base: str | None, step: str, scratch: Path, command: list[str],
                        remote: Remote | None, areas: dict[str, str] | None, record: dict,
                        budget: WaitBudget) -> Rerun:
    """The one rerun of the failing step, on the host it ran on. A timing-sensitive step that failed
    on a busy host is rerun after the load drops (the budget bounds all waiting); a rerun that fails
    while its host is busy is inconclusive. The timing steps run on the remote host when there is
    one (`REMOTE_AREAS`), so it is the quiet Linux host that is measured there."""
    selection = step_with_prerequisites(step)
    timing = is_timing_sensitive(step, areas)
    on_remote = remote is not None and (areas or {}).get(step) in REMOTE_AREAS

    def measure() -> Load | None:
        return remote_load(remote) if on_remote else local_load()

    def rerun(note: str = "", load: Load | None = None) -> Rerun:
        code = run_gate(worktree, base, selection, None, scratch / "rerun.json", command, remote)
        noisy = busy_after(measure) if timing and code and code not in INFRASTRUCTURE_EXITS else None
        return Rerun(code, measure, note, noisy, load)

    load = measure() if timing else None
    if load is not None:
        record["load"] = load.as_record()
    if load is None or not load.busy():
        return rerun(load=load)
    quiet = wait_until_quiet(measure, budget)
    note = f"{load.describe()} is busy; rerun after waiting"
    if quiet is not None:
        record["load_before_rerun"] = quiet.as_record()
        note += f" ({quiet.describe()}{', still busy' if quiet.busy() else ''})"
    return rerun(note, load)


def finish_after_flake(root: Path, worktree: Path, scratch: Path, record: dict, command: list[str],
                       remote: Remote | None, areas: dict[str, str] | None, load_note: str) -> bool:
    """A flaky step stopped the first run before the steps after it: run them, so that `flaky` says
    what else was verified. True when the record is settled (they passed, or could not run and the
    note says they are unverified); False when one failed, which is now `failed_step`."""
    log, base = Path(record["log"]), parent_of(root, record["sha"])
    code = run_unrun_steps(worktree, base, record, log, scratch, command, remote, areas)
    if code and code not in INFRASTRUCTURE_EXITS:
        if record["failed_step"] == EDITOR_STEP and lacks_measurement(log):
            code = settle_missing_measurement(worktree, base, record, log, scratch, command, remote)
        if code:
            return False
    tail = ("the steps after it ran and passed" if not code else
            f"the steps after it could not be run (exit {code}) and are unverified")
    if record["result"] != NEEDS_MEASUREMENT:  # that run is green for the search; its note says why
        flaked = ", ".join(flake["step"] for flake in record["flaky_steps"])
        record["result"] = "flaky"
        record["note"] = (f"step {flaked} passed when run again on the same commit; no commit is "
                          f"blamed, and {tail}{load_note}")
    return True


def investigate(root: Path, worktree: Path, scratch: Path, health: Path, record: dict,
                since: str | None, command: list[str], remote: Remote | None = None,
                persistent: bool = False) -> None:
    """Fill in the culprit of a red run: a flaky step blames nobody, and the steps it hid still run
    (one of them may fail in its turn, and is then judged the same way)."""
    sha = record["sha"]
    areas = gate_step_areas(worktree, command)
    budget = WaitBudget()
    while True:
        step = record.get("failed_step")
        if not step:
            record["note"] = "the failing step is unknown, so the culprit was not searched"
            return
        again = rerun_on_quiet_host(worktree, parent_of(root, sha), step, scratch, command, remote, areas,
                                    record, budget)
        if again.code in INFRASTRUCTURE_EXITS:  # a busy or unreachable host says nothing about a commit
            raise InfrastructureError(f"the rerun of {step} could not run (exit {again.code})")
        load_note = f" ({again.note})" if again.note else ""
        if again.code == 0:
            flaked = record.setdefault("flaky_steps", [])
            flaked.append(gate_flakes.flaky_step(step, record, again.load and again.load.as_record()))
            if finish_after_flake(root, worktree, scratch, record, command, remote, areas, load_note):
                return
            if record["failed_step"] in {flake["step"] for flake in flaked}:
                # The same step fails in the batch and passes alone (or the run left no step times
                # to tell it apart): judging it again would never end. Each pass adds a new step.
                record["result"] = "flaky"
                record["note"] = (f"step {record['failed_step']} fails in the full run and passes when run "
                                  "alone; the steps after it cannot be told apart, so no commit is blamed")
                return
            continue
        break
    if again.busy_after:
        record.setdefault("flaky_steps", []).append(
            gate_flakes.flaky_step(step, record, again.busy_after.as_record(), gate_flakes.INCONCLUSIVE))
        record["result"] = "flaky"
        record["load_after_rerun"] = again.busy_after.as_record()
        record["note"] = (f"step {step} failed again, but {again.busy_after.describe()} is busy: the result is "
                          f"inconclusive, no commit is blamed and the next pass measures it again{load_note}")
        return
    blame(root, worktree, scratch, health, record, since, command, remote, persistent, again, budget,
          is_timing_sensitive(step, areas))


def blame(root: Path, worktree: Path, scratch: Path, health: Path, record: dict, since: str | None,
          command: list[str], remote: Remote | None, persistent: bool, again: Rerun,
          budget: WaitBudget, timing: bool) -> None:
    """The failing step failed again: name the commit that broke it, or the candidates."""
    sha, step = record["sha"], record["failed_step"]
    commits = search_range(root, health, sha, since)
    if not commits:
        record["note"] = "no known green main to search from; pass --since"
    elif len(commits) == 1:
        record["culprit"] = commit_info(root, commits[0])
    else:
        result = locate_culprit(root, worktree, commits, step, command, scratch, remote, persistent,
                                again.measure if timing else None, budget)
        if result.culprit:
            record["culprit"] = commit_info(root, result.culprit)
        else:
            record["candidates"] = [commit_info(root, c) for c in result.candidates]
            record["note"] = "some commits cannot run the failing step alone"
    notes = [record.get("note"), f"the rerun also failed ({again.note})" if again.note else None]
    if record.get("flaky_steps"):
        flaked = ", ".join(flake["step"] for flake in record["flaky_steps"])
        notes.append(f"{flaked} flaked earlier in this run and passed when run again")
    if any(notes):
        record["note"] = "; ".join(note for note in notes if note)


def install_editor_meter(meter: Path | None, worktree: Path) -> str:
    """Copy the VM1 report into the worktree: `copied`, `missing` or `not-given`."""
    if meter is None:
        return "not-given"
    if not meter.is_file():
        print(f"post_merge: the editor measurement {meter} does not exist", flush=True)
        return "missing"
    target = worktree / EDITOR_REPORT
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(meter, target)
    return "copied"


def meter_measures(meter: Path | None, editor_hash: str) -> bool:
    """Whether the report at `meter` is a measurement of the editor files with this hash."""
    try:
        return meter is not None and json.loads(meter.read_text()).get("editor_tree_hash") == editor_hash
    except (OSError, ValueError, AttributeError):
        return False


def lacks_measurement(log: Path) -> bool:
    """Whether the editor check failed only for want of a valid measurement of this commit."""
    lines = log.read_text(errors="replace").splitlines() if log.exists() else []
    return any(line.startswith(MISSING_REPORT)
               or (line.startswith("editor-performance: ") and STALE_REPORT in line) for line in lines)


def run_unrun_steps(worktree: Path, base: str | None, record: dict, log: Path, scratch: Path,
                    command: list[str], remote: Remote | None,
                    areas: dict[str, str] | None = None) -> int:
    """The gate stops at its first failure: run the steps it never reached, add their times to the
    record, and set `failed_step` to the one that fails. UNSUPPORTED_STEP_EXIT when the run cannot
    say which steps it left out (the gate cannot list its steps, or the run left no step times)."""
    rest = gate_flakes.unrun_steps(areas or gate_step_areas(worktree, command), record["steps"])
    if rest is None:
        return UNSUPPORTED_STEP_EXIT
    if not rest:
        return 0
    ran = {step["id"] for step in record["steps"]}
    # A prerequisite that already ran is not taken again: its check would compare nothing.
    selection = ",".join(dict.fromkeys(i for r in rest for i in step_with_prerequisites(r).split(",")
                                       if i == r or i not in ran))
    later = scratch / "after-failure.json"
    later.unlink(missing_ok=True)
    code = run_gate(worktree, base, selection, log, later, command, remote)
    try:
        record["steps"] += json.loads(later.read_text())["latest"]["steps"]
    except (OSError, ValueError, KeyError):
        pass  # a gate that ran no step has no timings; the exit code says so
    if code and code not in INFRASTRUCTURE_EXITS:
        record["failed_step"] = failing_step(log, later, last=True)
    return code


def settle_missing_measurement(worktree: Path, base: str | None, record: dict, log: Path,
                               scratch: Path, command: list[str], remote: Remote | None) -> int:
    """Run what the editor check hid. 0 when all passed: the run is `needs-measurement`."""
    editor_hash = check_editor_performance.editor_tree_hash(worktree)  # before any probe moves HEAD
    code = run_unrun_steps(worktree, base, record, log, scratch, command, remote)
    if code in INFRASTRUCTURE_EXITS:
        raise InfrastructureError(f"the steps after {EDITOR_STEP} could not run (exit {code}); see {log}")
    if code:
        return code
    record.update(result=NEEDS_MEASUREMENT, editor_tree_hash=editor_hash, note=(
        f"no valid VM1 measurement of editor tree {editor_hash}: measure it on VM1 and pass the "
        "report with --editor-meter; every other step passed, and nothing is reverted"))
    return 0


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
        local = local_load()  # a run that passed on a loaded host is worth knowing about too
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


def read_record(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return {}


def measured_since(record: dict, meter: Path | None) -> bool:
    """A `needs-measurement` run is done again once `meter` measures the editor files it names."""
    return record.get("result") == NEEDS_MEASUREMENT and meter_measures(meter, record.get("editor_tree_hash", ""))


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
