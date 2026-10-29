"""The hosts a post-merge run uses (ADR 0089): the remote Linux host that runs the engine,
desktop, e2e and timing steps over ssh, and the load of the host a timing step runs on.

A timing-sensitive step is judged by the load of its host, and a run waits a bounded time for a
busy one to quiet down (`WaitBudget`). Callers that the tests replace (`local_load`,
`remote_load`, `wait_until_quiet`, `pause`) are reached through this module, not copied from it.
"""

from __future__ import annotations

import os
import shlex
import subprocess
import time
from dataclasses import dataclass
from typing import Callable

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
# Set to a command (split on whitespace) to replace `ssh -o BatchMode=yes`; the tests use it.
SSH_ENV = "SOYEHT_REMOTE_SSH"
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
