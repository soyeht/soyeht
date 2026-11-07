"""The hosts a gate run uses (ADR 0089): the remote Linux host that runs the engine, desktop, e2e
and timing steps over ssh (after the merge, and before it with scripts/gate_remote.py), how a
commit that is on no remote reaches it, and the load of the host a timing step runs on.

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
from pathlib import Path
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
HOST_PROBE_TIMEOUT_SECONDS = 120  # the ssh that asks whether the host holds a commit (it fetches)
SHIP_TIMEOUT_SECONDS = 300  # the ssh that receives a bundle
# Where on the host, under the checkout, a bundle waits for scripts/remote_gate.sh --bundle.
BUNDLE_DIRECTORY = ".cache/remote-gate"


@dataclass(frozen=True)
class Remote:
    host: str
    root: str  # a checkout of the repository on the host that has scripts/remote_gate.sh
    slot: str = ""  # a persistent worktree of its own on the host (remote_gate.sh --slot)

    def remote_path(self) -> str:
        """The root as the host's shell should read it: a leading `~` is the host's home."""
        if self.root == "~" or self.root.startswith("~/"):
            return '"$HOME"' + shlex.quote(self.root[1:])
        return shlex.quote(self.root)

    def slot_arguments(self) -> str:
        return f" --slot {shlex.quote(self.slot)}" if self.slot else ""

    def ssh(self, command: str) -> list[str]:
        ssh = os.environ.get(SSH_ENV, "ssh -o BatchMode=yes").split()
        # A login shell, so that the host's toolchain is on PATH. A missing checkout or script is
        # a problem of the host (exit 2), not a red main.
        script = (f"cd {self.remote_path()} || exit 2; "
                  f"[ -f scripts/remote_gate.sh ] || exit 2; {command}")
        return [*ssh, self.host, "bash -lc " + shlex.quote(script)]


class ShipError(RuntimeError):
    """A commit could not be sent to the host: a gate that cannot run, not a red commit."""


def host_has_commit(remote: Remote, sha: str) -> bool:
    """Whether the host's checkout holds the commit (it fetches `origin main` first). An
    unreachable host, or one that cannot answer, is a `ShipError`: nothing is silently skipped."""
    try:
        result = subprocess.run(remote.ssh(f"bash scripts/remote_gate.sh --has {sha}"),
                                capture_output=True, text=True, timeout=HOST_PROBE_TIMEOUT_SECONDS)
    except (subprocess.TimeoutExpired, OSError) as error:
        raise ShipError(f"cannot ask {remote.host} whether it holds {sha[:12]}: {error}") from error
    if result.returncode not in (0, 1):
        raise ShipError(f"cannot ask {remote.host} whether it holds {sha[:12]} "
                        f"(exit {result.returncode}: {result.stderr.strip() or 'no message'})")
    return result.returncode == 0


def ship_commit(remote: Remote, worktree: Path, base: str | None, scratch: Path) -> str | None:
    """Send the commit checked out in `worktree` to the host in a git bundle, for a commit that is
    on no remote (the head of a pull request). The bundle holds `base..HEAD` when the host has the
    base, else the whole history. Returns the bundle's path on the host (for `remote_gate.sh
    --bundle`, which removes it), or nothing when the host already holds the commit."""
    sha = subprocess.run(["git", "rev-parse", "HEAD"], cwd=worktree, capture_output=True, text=True,
                         check=True).stdout.strip()
    if host_has_commit(remote, sha):
        return None
    name = f"gate-{sha[:12]}{'-' + remote.slot if remote.slot else ''}.bundle"
    local, destination = scratch / name, f"{BUNDLE_DIRECTORY}/{name}"
    revisions = f"{base}..HEAD" if base and base != sha and host_has_commit(remote, base) else "HEAD"
    made = subprocess.run(["git", "bundle", "create", str(local), revisions], cwd=worktree,
                          capture_output=True, text=True)
    if made.returncode:
        raise ShipError(f"cannot bundle {revisions}: {made.stderr.strip()}")
    try:
        with local.open("rb") as handle:
            sent = subprocess.run(
                remote.ssh(f"mkdir -p {BUNDLE_DIRECTORY} && cat > {shlex.quote(destination)}"),
                stdin=handle, capture_output=True, text=True, timeout=SHIP_TIMEOUT_SECONDS)
    except (subprocess.TimeoutExpired, OSError) as error:
        drop_bundle(remote, destination)  # a part of it may have arrived
        raise ShipError(f"cannot send the bundle to {remote.host}: {error}") from error
    finally:
        local.unlink(missing_ok=True)
    if sent.returncode:
        drop_bundle(remote, destination)
        raise ShipError(f"cannot send the bundle to {remote.host} (exit {sent.returncode}: "
                        f"{sent.stderr.strip() or 'no message'})")
    return destination


def drop_bundle(remote: Remote, destination: str) -> None:
    """Remove a bundle from the host when `remote_gate.sh` never got to (a busy worktree, a failed ssh)."""
    try:
        subprocess.run(remote.ssh(f"rm -f {shlex.quote(destination)}"), capture_output=True,
                       timeout=LOAD_PROBE_TIMEOUT_SECONDS)
    except (subprocess.TimeoutExpired, OSError):
        pass  # the next bundle of this commit overwrites it, and remote_gate.sh removes it after use


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
