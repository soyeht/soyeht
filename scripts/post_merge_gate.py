"""Running the gate on a commit (ADR 0089): `check_all.sh --tier full` here, or split between the
remote host and this machine (`--remote`), the exit codes that mean the gate could not run, and
reading which step failed from the run's step times or its log.
"""

from __future__ import annotations

import json
import os
import re
import shlex
import signal
import subprocess
import tempfile
from contextlib import contextmanager
from pathlib import Path

from gate_worktree import git, scratch_root
from post_merge_hosts import (REMOTE_AREAS, REMOTE_BUSY_EXIT, SSH_FAILED_EXIT, Remote, ShipError, drop_bundle,
                              ship_commit)
from post_merge_records import write_json

FAILED_STEP = re.compile(r"^check_all: FAILED step (\S+)", re.MULTILINE)
# check_all.sh exit codes that mean the gate could not run, not that main is red.
UNSUPPORTED_STEP_EXIT = 78
INFRASTRUCTURE_EXIT = 2
INFRASTRUCTURE_EXITS = {INFRASTRUCTURE_EXIT, 64, 70, 71, UNSUPPORTED_STEP_EXIT}
# A step that only means something after another step of the same run.
PREREQUISITES = {"e2e:tcc-check": "e2e:tcc-snapshot"}
REMOTE_UNSUPPORTED_EXIT = UNSUPPORTED_STEP_EXIT


TO_STDOUT = Path("-")  # as a log: the gate's output stays on this process's standard output
# remote_gate.sh runs `engine:test` and `engine:test-changed` against the programs `engine:build`
# builds; without that build a test of the CLI fails for a reason that is not the commit's.
REMOTE_PREREQUISITES = {"engine:test": "engine:build", "engine:test-changed": "engine:build"}


@contextmanager
def opened(log: Path | None):
    """The output a gate process writes to: nowhere (no log), this process's own, or the log."""
    if log is None:
        yield subprocess.DEVNULL
    elif log == TO_STDOUT:
        yield None
    else:
        with log.open("ab") as handle:
            yield handle


def note(log: Path | None, message: str) -> None:
    """A line of this script's own in the log, apart from the gate's output."""
    if log == TO_STDOUT:
        print(f"gate: {message}", flush=True)
    elif log is not None:
        with log.open("ab") as handle:
            handle.write(f"gate: {message}\n".encode())


def start_process(arguments: list[str], cwd: Path, env: dict, output) -> subprocess.Popen:
    """Start in its own process group, so a signal to this script stops the whole gate."""
    return subprocess.Popen(arguments, cwd=cwd, env=env, stdout=output,
                            stderr=subprocess.STDOUT if output != subprocess.DEVNULL else subprocess.DEVNULL,
                            start_new_session=True)


def stop_process(process: subprocess.Popen) -> None:
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
    process.wait()


def wait_process(process: subprocess.Popen) -> int:
    try:
        return process.wait()
    except BaseException:
        stop_process(process)
        raise


def run_process(arguments: list[str], cwd: Path, env: dict, output) -> int:
    return wait_process(start_process(arguments, cwd, env, output))


def commit_event(worktree: Path) -> dict:
    """The pull request event of the commit checked out in `worktree`. A squash merge makes the
    pull request's title the subject and its body the rest of the message, so the checks that
    read the event (the `Verification change:` waiver of `hardening_gate.py`, the forbidden
    terms) see the same text after the merge as before it."""
    subject, _, body = git(worktree, "log", "-1", "--format=%B", "HEAD").partition("\n")
    return {"pull_request": {"title": subject, "body": body.strip()}}


def run_local_steps(worktree: Path, tier: str, base: str | None, only: str | None, event: Path | None,
                    log: Path | None, timings: Path, command: list[str]) -> int:
    """`check_all.sh` here. The event (the pull request's title and body) goes to every step, as
    in a pull request run."""
    arguments = [*command, "--tier", tier]
    if only:
        arguments += ["--only", only]
    if base:
        arguments += ["--base", base]
    if event:
        arguments += ["--event-file", str(event)]
    with opened(log) as output:
        return run_process(arguments, worktree, {**os.environ, "SOYEHT_GATE_TIMINGS": str(timings)}, output)


def run_local_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
                   timings: Path, command: list[str]) -> int:
    # The event is outside the worktree: the checks scan its untracked files.
    with tempfile.TemporaryDirectory(dir=scratch_root()) as directory:
        event = Path(directory) / "event.json"
        write_json(event, commit_event(worktree))
        return run_local_steps(worktree, "full", base, only, event, log, timings, command)


def gate_step_areas(worktree: Path, command: list[str], tier: str = "full", only: str | None = None,
                    base: str | None = None) -> dict[str, str] | None:
    """{step id: area} of the steps the tier would run (those of `only` when given), or None when
    the commit's gate cannot list its steps."""
    arguments = [*command, "--tier", tier, "--list"]
    if only:
        arguments += ["--only", only]
    if base:
        arguments += ["--base", base]
    result = subprocess.run(arguments, cwd=worktree, capture_output=True, text=True)
    if result.returncode:
        return None
    rows = [line.split() for line in result.stdout.splitlines()]
    return {row[2]: row[0] for row in rows if len(row) == 3}


def remote_steps_timings(remote: Remote) -> list[dict]:
    """The per-step times of the last remote run, or nothing when the host cannot say."""
    result = subprocess.run(remote.ssh(f"bash scripts/remote_gate.sh --print-timings{remote.slot_arguments()}"),
                            capture_output=True, text=True)
    try:
        return json.loads(result.stdout)["latest"]["steps"] if result.returncode == 0 else []
    except (ValueError, KeyError, TypeError):
        return []


def remote_gate_arguments(remote: Remote, sha: str, ids: list[str], base: str | None,
                          bundle: str | None, engine_test_args: str) -> str:
    arguments = f"bash scripts/remote_gate.sh --sha {sha} --steps {','.join(ids)}{remote.slot_arguments()}"
    for flag, value in (("--base", base), ("--bundle", bundle), ("--engine-test-args", engine_test_args)):
        if value:
            arguments += f" {flag} {shlex.quote(value)}"
    return arguments


def remote_exit(remote: Remote, code: int, log: Path | None) -> int:
    """The gate's exit code, or the infrastructure one (said in the log) when the host could not run it."""
    reasons = {SSH_FAILED_EXIT: f"cannot reach {remote.host} over ssh",
               REMOTE_BUSY_EXIT: f"the worktree on {remote.host} is busy with another run"}
    if code in reasons:
        note(log, f"{reasons[code]} (exit {code}); the gate did not run")
        return INFRASTRUCTURE_EXIT
    if code == INFRASTRUCTURE_EXIT:
        note(log, f"{remote.host} could not run the gate (exit {code}); see the output above")
    return code


def split_gate(worktree: Path, base: str | None, only: str | None, log: Path | None, timings: Path,
               command: list[str], remote: Remote, *, tier: str = "full", event: Path | None = None,
               ship: bool = False, engine_test_args: str = "", concurrent: bool = False
               ) -> tuple[int, list[dict]]:
    """The remote areas run on the host and the rest here, and the exit code and the steps with
    their times. By default the remote steps run first and the first failure ends the run; with
    `concurrent` both halves run at once and both are reported. With `ship` the commit travels in
    a git bundle (it is on no remote, as the head of a pull request); an unreachable host is a
    clear infrastructure failure, never a pass."""
    areas = gate_step_areas(worktree, command, tier, only, base)
    if areas is None:
        return REMOTE_UNSUPPORTED_EXIT, []
    wanted = only.split(",") if only else list(areas)
    remote_ids = [i for i in areas if i in wanted and areas[i] in REMOTE_AREAS]
    local_ids = [i for i in areas if i in wanted and areas[i] not in REMOTE_AREAS]
    local_ids += [i for i in wanted if i not in areas]  # the local gate reports an unknown step
    remote_ids += [REMOTE_PREREQUISITES[i] for i in remote_ids
                   if i in REMOTE_PREREQUISITES and REMOTE_PREREQUISITES[i] not in remote_ids]
    sha = git(worktree, "rev-parse", "HEAD")
    timings.unlink(missing_ok=True)
    local_timings = timings.with_name(timings.name + ".local")
    local_timings.unlink(missing_ok=True)
    process: subprocess.Popen | None = None
    bundle = None
    remote_code = local_code = 0
    try:
        if remote_ids:
            try:
                bundle = ship_commit(remote, worktree, base, timings.parent) if ship else None
            except ShipError as error:
                note(log, str(error))
                return INFRASTRUCTURE_EXIT, []
            arguments = remote_gate_arguments(remote, sha, remote_ids, base, bundle, engine_test_args)
            with opened(log) as output:
                process = start_process(remote.ssh(arguments), worktree, os.environ.copy(), output)
                if not concurrent:
                    remote_code, process = wait_process(process), None
                if not remote_code and local_ids:
                    local_code = run_local_steps(worktree, tier, base, ",".join(local_ids), event, log,
                                                 local_timings, command)
                if process:
                    remote_code, process = wait_process(process), None
        elif local_ids:
            local_code = run_local_steps(worktree, tier, base, ",".join(local_ids), event, log,
                                         local_timings, command)
    finally:
        if process:
            stop_process(process)
        if bundle:
            drop_bundle(remote, bundle)
    steps = [{**step, "host": remote.host} for step in remote_steps_timings(remote)] if remote_ids else []
    try:
        steps += json.loads(local_timings.read_text())["latest"]["steps"]
    except (OSError, ValueError, KeyError):
        pass  # a gate that failed to start has no timings; the exit code says so
    return remote_exit(remote, remote_code, log) or local_code, steps


def run_split_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
                   timings: Path, command: list[str], remote: Remote) -> int:
    """After the merge: the remote areas run on the host, then the rest here; the first failure
    ends the run."""
    code, steps = split_gate(worktree, base, only, log, timings, command, remote)
    if steps:
        write_json(timings, {"latest": {"steps": steps}})
    return code


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
