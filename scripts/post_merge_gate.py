"""Running the gate on a commit (ADR 0089): `check_all.sh --tier full` here, or split between the
remote host and this machine (`--remote`), the exit codes that mean the gate could not run, and
reading which step failed from the run's step times or its log.
"""

from __future__ import annotations

import json
import os
import re
import signal
import subprocess
import tempfile
from pathlib import Path

from gate_worktree import git, scratch_root
from post_merge_hosts import REMOTE_AREAS, REMOTE_BUSY_EXIT, SSH_FAILED_EXIT, Remote
from post_merge_records import write_json

FAILED_STEP = re.compile(r"^check_all: FAILED step (\S+)", re.MULTILINE)
# check_all.sh exit codes that mean the gate could not run, not that main is red.
UNSUPPORTED_STEP_EXIT = 78
INFRASTRUCTURE_EXIT = 2
INFRASTRUCTURE_EXITS = {INFRASTRUCTURE_EXIT, 64, 70, 71, UNSUPPORTED_STEP_EXIT}
# A step that only means something after another step of the same run.
PREREQUISITES = {"e2e:tcc-check": "e2e:tcc-snapshot"}
REMOTE_UNSUPPORTED_EXIT = UNSUPPORTED_STEP_EXIT


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
