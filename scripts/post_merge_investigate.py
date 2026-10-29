"""What the gate does with a failing step (ADR 0089): rerun it once on the host it ran on, call a
pass a flake, run the steps a failed run never reached, and otherwise blame a commit.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

import check_editor_performance
import gate_flakes
import post_merge_hosts as hosts
from gate_worktree import InfrastructureError
from post_merge_culprit import locate_culprit, search_range
from post_merge_editor import EDITOR_STEP, NEEDS_MEASUREMENT, lacks_measurement
from post_merge_gate import (INFRASTRUCTURE_EXITS, UNSUPPORTED_STEP_EXIT, failing_step, gate_step_areas,
                             run_gate, step_with_prerequisites)
from post_merge_hosts import REMOTE_AREAS, Load, Remote, WaitBudget, busy_after, is_timing_sensitive
from post_merge_records import commit_info, parent_of

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
        return hosts.remote_load(remote) if on_remote else hosts.local_load()

    def rerun(note: str = "", load: Load | None = None) -> Rerun:
        code = run_gate(worktree, base, selection, None, scratch / "rerun.json", command, remote)
        noisy = busy_after(measure) if timing and code and code not in INFRASTRUCTURE_EXITS else None
        return Rerun(code, measure, note, noisy, load)

    load = measure() if timing else None
    if load is not None:
        record["load"] = load.as_record()
    if load is None or not load.busy():
        return rerun(load=load)
    quiet = hosts.wait_until_quiet(measure, budget)
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
