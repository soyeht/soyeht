"""The culprit search of a red main (ADR 0089): a binary search of the first-parent commits since
the last green main, probing each with only the failing step (`check_all.sh --only STEP`). It
starts with a control run: the same probe on the last green main itself, on today's host. A step
that fails there too is the host's or the environment's failure, and no commit is blamed.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

import post_merge_hosts as hosts
from gate_worktree import clean_worktree, git
from post_merge_editor import EDITOR_STEP, lacks_measurement
from post_merge_gate import INFRASTRUCTURE_EXITS, run_gate, step_with_prerequisites
from post_merge_hosts import Load, Remote, WaitBudget, busy_after
from post_merge_records import parent_of

GOOD, BAD, SKIP = "good", "bad", "skip"


@dataclass
class Search:
    culprit: str | None
    candidates: list[str] = field(default_factory=list)
    probes: dict[str, str] = field(default_factory=dict)
    control: str = GOOD  # the probe of the last green main: BAD is the host's failure, SKIP unknown


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


def supports_only(root: Path, sha: str) -> bool:
    result = subprocess.run(["git", "show", f"{sha}:scripts/check_all.sh"], cwd=root,
                            capture_output=True, text=True)
    return result.returncode == 0 and "--only" in result.stdout


def last_green(health: Path, since: str | None) -> str | None:
    green_file = health / "last_green.json"
    if since is None and green_file.exists():
        return json.loads(green_file.read_text())["sha"]
    return since


def search_range(root: Path, green: str | None, sha: str) -> list[str]:
    if green is None:
        return []
    return git(root, "rev-list", "--first-parent", "--reverse", f"{green}..{sha}", check=False).split()


def locate_culprit(root: Path, worktree: Path, green: str, commits: list[str], step: str,
                   command: list[str], scratch: Path, remote: Remote | None = None,
                   persistent: bool = False, measure: Callable[[], Load | None] | None = None,
                   budget: WaitBudget | None = None) -> Search:
    """The control run first: `step` on `green`, the last green main, with the probe every commit
    gets. Only when it passes there does the search over `commits` (oldest to newest, the newest
    being the failing head) run, and only then may it name a culprit. A control that fails is the
    host's failure and one that cannot run is unknown: neither blames a commit.

    `measure` (a timing step) reads the load of the host the probes run on: a probe waits for it
    to be quiet, and one that fails on a host that is (or turned) busy is skipped, so a busy host
    never blames a commit; one that passes is good wherever it ran."""
    probe = probe_of(root, worktree, step, command, scratch, remote, persistent, measure, budget)
    control = probe(green)
    if control != GOOD:
        return Search(None, [], {green: control}, control)
    return find_culprit(commits, probe)


def probe_of(root: Path, worktree: Path, step: str, command: list[str], scratch: Path,
             remote: Remote | None, persistent: bool, measure: Callable[[], Load | None] | None,
             budget: WaitBudget | None) -> Callable[[str], str]:
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
            load = hosts.wait_until_quiet(measure, budget or WaitBudget())
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

    return probe
