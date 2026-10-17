#!/usr/bin/env python3
"""Run the full gate on a main commit and find the culprit when it is red (ADR 0089).

    post_merge.py [--sha COMMIT] [--health-dir DIR] [--keep] [--force]
                  [--remote HOST --remote-root PATH] [--editor-meter REPORT] [--worktree DIR]

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

With `--remote HOST --remote-root PATH` the steps of the areas that run on Linux (`REMOTE_AREAS`:
engine, desktop, e2e) run on HOST over ssh, through `scripts/remote_gate.sh` of the checkout at
PATH (docs/dev-local.md), and the other steps (the Swift packages, the Mac and iOS apps, the
editor measurement, the dictation tests, the timing tests, the `always` checks) run here. The
remote steps run first; a failure there stops the run, as a local failure does.

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
from gate_base import main_checkout, main_health_dir

GOOD, BAD, SKIP = "good", "bad", "skip"
FAILED_STEP = re.compile(r"^check_all: FAILED step (\S+)", re.MULTILINE)
# check_all.sh exit codes that mean the gate could not run, not that main is red.
UNSUPPORTED_STEP_EXIT = 78
INFRASTRUCTURE_EXIT = 2
INFRASTRUCTURE_EXITS = {INFRASTRUCTURE_EXIT, 64, 70, 71, UNSUPPORTED_STEP_EXIT}
# A step that only means something after another step of the same run.
PREREQUISITES = {"e2e:tcc-check": "e2e:tcc-snapshot"}
WORKTREE_PREFIX = "soyeht-post-merge-"
# Where a run keeps its worktree. Unset, the system temporary directory; a test names its own so
# that two gates running the tests at once never remove each other's worktrees.
SCRATCH_ROOT_ENV = "SOYEHT_POST_MERGE_TMP"
BUSY_EXIT = 75
# The editor measurement (ADR 0092): the VM1 report is copied into the worktree, and a check that
# fails only because it is missing or stale is not a red main.
EDITOR_METER_ENV = "SOYEHT_EDITOR_METER"
EDITOR_STEP = "editor-performance:check"
EDITOR_REPORT = check_editor_performance.VM_REPORT.relative_to(check_editor_performance.ROOT)
NEEDS_MEASUREMENT = "needs-measurement"
NEEDS_MEASUREMENT_EXIT = 4
# What check_editor_performance.py prints for an absent report and for one of other editor files.
MISSING_REPORT = "editor-performance: missing "
STALE_REPORT = "editor paths changed; measure this editor version again on VM1"
# A persistent worktree (--worktree) keeps its cargo targets between runs. The Swift `.build`
# directories are cleaned: SwiftPM's incremental build misses cross-module changes across checkouts.
WORKTREE_ENV = "SOYEHT_POST_MERGE_WORKTREE"
# Xcode's derived data and the old reports of `.cache` go too, for the same reason; the package
# checkouts and the VM1 report stay.
KEPT_BETWEEN_RUNS = ("engine/target", "apps/desktop/target", ".cache/apple-packages",
                     str(EDITOR_REPORT))
# Written in the git directory of a worktree this script created: only such a worktree is ever
# checked out forcibly and cleaned.
OWNED_MARKER = "soyeht-post-merge-owned"
# A failure of the Rust half of the editor check, run here when the VM1 report is missing or stale.
EDITOR_RUST_STEP = "editor-performance:rust"
EDITOR_RUST_REPORT = check_editor_performance.RUST_REPORT.relative_to(check_editor_performance.ROOT)
# The areas whose steps run on the remote Linux host; every other step runs on this machine.
REMOTE_AREAS = ("engine", "desktop", "e2e")
# Set to a command (split on example) to replace `ssh -o BatchMode=yes`; the tests use it.
SSH_ENV = "SOYEHT_REMOTE_SSH"
REMOTE_UNSUPPORTED_EXIT = UNSUPPORTED_STEP_EXIT
SSH_FAILED_EXIT = 255  # ssh itself could not connect or run the command


@dataclass(frozen=True)
class Remote:
    host: str
    root: str  # a checkout of the repository on the host that has scripts/remote_gate.sh

    def ssh(self, command: str) -> list[str]:
        ssh = os.environ.get(SSH_ENV, "ssh -o BatchMode=yes").split()
        # A login shell, so that the host's toolchain is on PATH. A missing checkout or script is
        # a problem of the host (exit 2), not a red main.
        script = (f"cd {shlex.quote(self.root)} || exit 2; "
                  f"[ -f scripts/remote_gate.sh ] || exit 2; {command}")
        return [*ssh, self.host, "bash -lc " + shlex.quote(script)]


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


def run_local_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
                   timings: Path, command: list[str]) -> int:
    arguments = [*command, "--tier", "full"]
    if only:
        arguments += ["--only", only]
    if base:
        arguments += ["--base", base]
    environment = {**os.environ, "SOYEHT_GATE_TIMINGS": str(timings)}
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
        if code == SSH_FAILED_EXIT:
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
                   persistent: bool = False) -> Search:
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
        code = run_gate(worktree, parent_of(root, commit), selection, probe_log, scratch / "probe.json",
                        command, remote)
        print(f"post_merge: {commit[:12]} step {selection}: exit {code}", flush=True)
        if code in INFRASTRUCTURE_EXITS or (code and probe_log and lacks_measurement(probe_log)):
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


def investigate(root: Path, worktree: Path, scratch: Path, health: Path, record: dict,
                since: str | None, command: list[str], remote: Remote | None = None,
                persistent: bool = False) -> None:
    """Fill in the culprit of a red run: a flaky step blames nobody."""
    sha, step = record["sha"], record["failed_step"]
    if step == EDITOR_RUST_STEP:  # not a step of the gate: it cannot be rerun or probed alone
        return
    if not step:
        record["note"] = "the failing step is unknown, so the culprit was not searched"
        return
    again = run_gate(worktree, parent_of(root, sha), step_with_prerequisites(step), None,
                     scratch / "rerun.json", command, remote)
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
        result = locate_culprit(root, worktree, commits, step, command, scratch, remote, persistent)
        if result.culprit:
            record["culprit"] = commit_info(root, result.culprit)
        else:
            record["candidates"] = [commit_info(root, c) for c in result.candidates]
            record["note"] = "some commits cannot run the failing step alone"


def scratch_root() -> Path:
    return Path(os.environ.get(SCRATCH_ROOT_ENV) or tempfile.gettempdir())


def remove_stale_worktrees(root: Path, keep: Path | None = None) -> None:
    """Leftovers of a run that was killed; the lock says no other run is alive. The persistent
    worktree (`keep`) is not one, wherever it is."""
    git(root, "worktree", "prune", check=False)
    for stale in scratch_root().glob(f"{WORKTREE_PREFIX}*"):
        if keep and (stale.resolve() == keep or stale.resolve() in keep.parents):
            continue
        shutil.rmtree(stale, ignore_errors=True)
    git(root, "worktree", "prune", check=False)


def clean_worktree(worktree: Path) -> None:
    """Drop everything the last run left except the build directories (`KEPT_BETWEEN_RUNS`)."""
    keep = [argument for path in KEPT_BETWEEN_RUNS for argument in ("-e", path)]
    git(worktree, "clean", "-fdxq", *keep)


def own_checkout() -> Path:
    """The checkout this script runs from."""
    return Path(__file__).resolve().parents[1]


def owned_git_dir(root: Path, worktree: Path) -> Path:
    """The git directory of a worktree that this script created, or an InfrastructureError.

    Read from the `.git` file, so that it works when git itself fails in the worktree."""
    pointer = worktree / ".git"
    try:
        target = pointer.read_text().strip() if pointer.is_file() else ""
    except OSError:
        target = ""
    if not target.startswith("gitdir:"):
        raise InfrastructureError(f"{worktree} is not a linked worktree")
    git_dir = Path(target.removeprefix("gitdir:").strip()).resolve()
    common = Path(git(root, "rev-parse", "--path-format=absolute", "--git-common-dir")).resolve()
    if git_dir.parent != common / "worktrees":
        raise InfrastructureError(f"{worktree} is not a linked worktree of this repository")
    if not (git_dir / OWNED_MARKER).is_file():
        raise InfrastructureError(
            f"{worktree} was not created by post_merge.py; refusing to check out and clean it")
    return git_dir


def create_persistent_worktree(root: Path, worktree: Path, sha: str) -> None:
    if worktree.exists() and any(worktree.iterdir()):
        raise InfrastructureError(f"{worktree} exists and is not a worktree; refusing to use it")
    worktree.parent.mkdir(parents=True, exist_ok=True)
    git(root, "worktree", "add", "--detach", str(worktree), sha)
    git_dir = Path(git(worktree, "rev-parse", "--path-format=absolute", "--git-dir"))
    (git_dir / OWNED_MARKER).write_text("created by scripts/post_merge.py; checked out and cleaned there\n")


def prepare_persistent_worktree(root: Path, worktree: Path, sha: str) -> None:
    """Reuse `worktree` for `sha`, or create it.

    Only a worktree this script created is reused (the marker): the force and the clean below
    must never touch the main checkout, the checkout this script runs from, or anyone's work. A
    worktree that fails to prepare (a killed git left its index locked, say) is recreated."""
    resolved = worktree.resolve()
    if root.resolve() in (resolved, *resolved.parents):
        raise InfrastructureError(f"{worktree} is the main checkout or inside it")
    own = own_checkout()
    if resolved in (own, *own.parents):
        raise InfrastructureError(f"{worktree} contains the checkout this script runs from")
    try:
        if (worktree / ".git").exists():
            git_dir = owned_git_dir(root, worktree)
            (git_dir / "index.lock").unlink(missing_ok=True)  # the run lock is held: no git is running
            try:
                git(worktree, "checkout", "--quiet", "--detach", "--force", sha)
                clean_worktree(worktree)
            except RuntimeError as error:
                print(f"post_merge: recreating the worktree {worktree}: {error}", flush=True)
                git(root, "worktree", "remove", "--force", str(worktree), check=False)
                shutil.rmtree(worktree, ignore_errors=True)
                git(root, "worktree", "prune")
                create_persistent_worktree(root, worktree, sha)
        else:
            create_persistent_worktree(root, worktree, sha)
    except (RuntimeError, OSError) as error:
        raise InfrastructureError(f"cannot prepare the worktree {worktree}: {error}") from None


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


def run_steps_after(worktree: Path, base: str | None, step: str, log: Path, timings: Path,
                    command: list[str], remote: Remote | None) -> int:
    """The gate stops at its first failure: run the steps listed after `step` as well."""
    areas = gate_step_areas(worktree, command)
    if areas is None:
        return UNSUPPORTED_STEP_EXIT
    ids = list(areas)
    rest = ids[ids.index(step) + 1:] if step in ids else []
    if not rest:
        return 0
    selection = ",".join(dict.fromkeys(i for r in rest for i in step_with_prerequisites(r).split(",")))
    return run_gate(worktree, base, selection, log, timings, command, remote)


def rust_editor_failures(worktree: Path, log: Path) -> list[str]:
    """The Rust half of `check_editor_performance.check()`: the same benchmark and limits."""
    report = worktree / EDITOR_RUST_REPORT
    command = ["cargo", "run", "--locked", "--release", "--quiet", "-p", "soyeht-editor",
               "--example", "editor_performance", "--", "--report", str(report)]
    try:
        # The example writes the report without creating its directory, and `.cache` is ignored.
        report.parent.mkdir(parents=True, exist_ok=True)
        code = run_process_logged(command, worktree / "engine", log)
    except OSError as error:
        raise InfrastructureError(f"cannot run the Rust editor benchmark: {error}") from None
    if code:
        return [f"the benchmark exited with {code}"]
    try:
        rust = json.loads(report.read_text())
        return [f"{key}={rust[key]} ms >= {limit} ms"
                for key, limit in check_editor_performance.RUST_LIMITS.items() if float(rust[key]) >= limit]
    except (OSError, ValueError, KeyError, TypeError) as error:
        return [f"the benchmark report cannot be read: {error}"]


def settle_missing_measurement(worktree: Path, base: str | None, record: dict, log: Path,
                               scratch: Path, command: list[str], remote: Remote | None) -> int:
    """Run what the editor check hid. 0 when all passed: the run is `needs-measurement`."""
    editor_hash = check_editor_performance.editor_tree_hash(worktree)  # before any probe moves HEAD
    later = scratch / "after-editor.json"
    code = run_steps_after(worktree, base, EDITOR_STEP, log, later, command, remote)
    if code in INFRASTRUCTURE_EXITS:
        raise InfrastructureError(f"the steps after {EDITOR_STEP} could not run (exit {code}); see {log}")
    try:
        record["steps"] += json.loads(later.read_text())["latest"]["steps"]
    except (OSError, ValueError, KeyError):
        pass  # a gate that ran no step has no timings; the exit code says so
    if code:
        record["failed_step"] = failing_step(log, later, last=True)
        return code
    # The check stopped at the missing report before its Rust half: that half is main's own.
    failures = rust_editor_failures(worktree, log)
    if failures:
        record.update(failed_step=EDITOR_RUST_STEP, note="the Rust editor check failed: " + "; ".join(failures))
        return 1
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
        log.write_bytes(b"")  # a run for a commit that is recorded already (--force) starts afresh
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
        if remote:
            record["remote"] = remote.host
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
        if record["result"] == "pass":
            write_json(health / "last_green.json", {"sha": sha, "at": record["started_at"]})
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
                        help="run the engine, desktop and e2e steps on this host over ssh")
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
