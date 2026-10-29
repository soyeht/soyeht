"""The editor measurement in the post-merge gate (ADR 0092): the VM1 report is copied into the
run's worktree, and a check that fails only because that report is missing or stale is not a red
main but a `needs-measurement` run, which counts as green for the culprit search.
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path

from gate_worktree import EDITOR_REPORT

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


def measured_since(record: dict, meter: Path | None) -> bool:
    """A `needs-measurement` run is done again once `meter` measures the editor files it names."""
    return record.get("result") == NEEDS_MEASUREMENT and meter_measures(meter, record.get("editor_tree_hash", ""))
