"""Running the gate on a commit (ADR 0089): `check_all.sh --tier full` here, or split between the
remote host and this machine (`--remote`), the exit codes that mean the gate could not run, and
reading which step failed from the run's step times or its log.
"""

from __future__ import annotations

import json
import os
import re
import select
import shlex
import signal
import subprocess
import sys
import tempfile
import threading
import time
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
# What a step reads from earlier steps of the same run: the build of the programs it starts, or the
# snapshot it compares with. A rerun on any commit (the culprit search, the flake check) takes them
# first, so a step is never judged against the programs of another commit that the worktree still
# holds (its target directory is kept between probes). test_gate_list holds every step that starts
# the engine's programs to this table.
BUILT_PROGRAMS = ("engine:build-product", "engine:build-test-engine")
PREREQUISITES: dict[str, tuple[str, ...]] = {
    "e2e:tcc-check": ("e2e:tcc-snapshot",),
    **{f"e2e:{suite}": BUILT_PROGRAMS for suite in
       ("core", "agents", "tasks", "editor", "web", "writing", "real-acp", "real-agent-messaging",
        "real-harnesses")},
    "swift:test-shared": ("swift:engine-binaries",),
}
REMOTE_UNSUPPORTED_EXIT = UNSUPPORTED_STEP_EXIT


TO_STDOUT = Path("-")  # as a log: the gate's output stays on this process's standard output
# remote_gate.sh runs `engine:test` and `engine:test-changed` against the programs `engine:build`
# builds; without that build a test of the CLI fails for a reason that is not the commit's.
REMOTE_PREREQUISITES = {"engine:test": "engine:build", "engine:test-changed": "engine:build"}
# What `check_all.sh` prints when the host lacks the Windows target: the step passes, unlinted.
WINDOWS_LINT_SKIPPED = b"skipped the Windows lint"
# After ssh exits, the longest the gate reads what is left in its pipe (a process that inherited the
# pipe may hold it open for ever).
DRAIN_SECONDS = 5


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
    return subprocess.Popen(arguments, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=output,
                            stderr=subprocess.STDOUT if output != subprocess.DEVNULL else subprocess.DEVNULL,
                            start_new_session=True)


class Pump(threading.Thread):
    """Copies the output of the remote gate to its sink, and notes the line that says the host
    skipped the Windows lint (no sink: the output is dropped). It reads the pipe's descriptor with
    a timeout so that it can be told to finish (`finish`): a process that inherited ssh's output
    (a ControlPersist master, a ProxyCommand child) keeps the pipe open after ssh exits, and the
    gate must not wait for it beyond `DRAIN_SECONDS`. A sink that fails (a closed pipe, a full
    disk) is dropped and the output is still drained, or ssh would block on a full pipe."""

    def __init__(self, stream, sink) -> None:
        super().__init__(daemon=True)
        self.stream, self.sink, self.windows_lint_skipped = stream, sink, False
        self.finishing = threading.Event()

    def finish(self) -> None:
        self.finishing.set()

    def write(self, chunk: bytes) -> None:
        if self.sink is None:
            return
        try:
            self.sink.write(chunk)
            self.sink.flush()
        except (OSError, ValueError):
            self.sink = None  # the output is lost from here; the scan and the drain go on

    def run(self) -> None:
        descriptor, tail, deadline = self.stream.fileno(), b"", None
        while True:
            if self.finishing.is_set() and deadline is None:
                deadline = time.monotonic() + DRAIN_SECONDS
            if deadline is not None and time.monotonic() > deadline:
                return
            try:
                ready, _, _ = select.select([descriptor], [], [], 0.1)
                chunk = os.read(descriptor, 65536) if ready else None
            except (OSError, ValueError):
                return
            if chunk == b"":
                return
            if chunk is None:
                if deadline is not None:
                    return  # finishing, and nothing more to read for now
                continue
            window = tail + chunk
            self.windows_lint_skipped = self.windows_lint_skipped or WINDOWS_LINT_SKIPPED in window
            tail = window[-len(WINDOWS_LINT_SKIPPED):]
            self.write(chunk)


def start_remote(arguments: list[str], cwd: Path, output) -> subprocess.Popen:
    """The ssh to the host, its output read by a `Pump` into `output` (`opened`'s kinds)."""
    process = subprocess.Popen(arguments, cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, start_new_session=True)
    if output is None:
        sys.stdout.flush()
        sink = sys.stdout.buffer
    else:
        sink = None if output == subprocess.DEVNULL else output
    process.pump = Pump(process.stdout, sink)
    process.pump.start()
    return process


def end_pump(process: subprocess.Popen) -> None:
    """Let the pump drain what ssh wrote, for no more than `DRAIN_SECONDS`, and close the pipe."""
    pump = getattr(process, "pump", None)
    if pump:
        pump.finish()
        pump.join(timeout=DRAIN_SECONDS * 2)
        if not pump.is_alive():
            process.stdout.close()


def stop_process(process: subprocess.Popen) -> None:
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
    process.wait()
    end_pump(process)


def wait_process(process: subprocess.Popen) -> int:
    try:
        code = process.wait()
        end_pump(process)
        return code
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


@contextmanager
def commit_event_file(worktree: Path):
    """The commit's event as a file outside the worktree: the checks scan its untracked files."""
    with tempfile.TemporaryDirectory(dir=scratch_root()) as directory:
        event = Path(directory) / "event.json"
        write_json(event, commit_event(worktree))
        yield event


def run_local_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
                   timings: Path, command: list[str]) -> int:
    with commit_event_file(worktree) as event:
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
    arguments = f"bash scripts/remote_gate.sh --sha {sha} --steps {shlex.quote(','.join(ids))}{remote.slot_arguments()}"
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


def divide_steps(areas: dict[str, str]) -> tuple[list[str], list[str]]:
    """The ids that run on the host (with the steps they need there) and the ids that run here."""
    remote_ids = [i for i in areas if areas[i] in REMOTE_AREAS]
    needed = [REMOTE_PREREQUISITES[i] for i in remote_ids if i in REMOTE_PREREQUISITES]
    return list(dict.fromkeys([*remote_ids, *needed])), [i for i in areas if areas[i] not in REMOTE_AREAS]


def plan_problem(areas: dict[str, str] | None, only: str | None) -> str | None:
    """Why the steps cannot be trusted to be the gate, or nothing. An empty plan or a misspelled id
    would otherwise run nothing and pass."""
    if areas is None:
        return "the gate cannot list its steps"
    if not areas:
        return "the gate's plan selects no step; refusing to pass a gate that checked nothing"
    missing = [i for i in (only.split(",") if only else []) if i not in areas]
    return f"--only names no such step: {', '.join(missing)}" if missing else None


def combine_codes(remote_code: int, local_code: int) -> int:
    """One exit code for the two halves: a gate that could not run (an infrastructure code of either
    half) outranks a step that failed."""
    for code in (remote_code, local_code):
        if code in INFRASTRUCTURE_EXITS:
            return code
    return remote_code or local_code


def split_gate(worktree: Path, base: str | None, only: str | None, log: Path | None, timings: Path,
               command: list[str], remote: Remote, *, tier: str = "full", event: Path | None = None,
               ship: bool = False, engine_test_args: str = "", concurrent: bool = False
               ) -> tuple[int, list[dict]]:
    """The remote areas run on the host and the rest here, and the exit code and the steps with
    their times. By default the remote steps run first and the first failure ends the run; with
    `concurrent` both halves run at once and both are reported. With `ship` the commit travels in
    a git bundle (it is on no remote, as the head of a pull request); an unreachable host, an empty
    plan and an unknown step are clear failures, never a pass."""
    areas = gate_step_areas(worktree, command, tier, only, base)
    problem = plan_problem(areas, only)
    if problem:
        note(log, problem)
        return REMOTE_UNSUPPORTED_EXIT, []
    wanted = only.split(",") if only else list(areas)
    remote_ids, local_ids = divide_steps({i: area for i, area in areas.items() if i in wanted})
    sha = git(worktree, "rev-parse", "HEAD")
    timings.unlink(missing_ok=True)
    local_timings = timings.with_name(timings.name + ".local")
    local_timings.unlink(missing_ok=True)
    process: subprocess.Popen | None = None
    pump: Pump | None = None
    bundle = None
    remote_code = local_code = 0

    def local() -> int:
        return run_local_steps(worktree, tier, base, ",".join(local_ids), event, log, local_timings, command)

    try:
        if remote_ids:
            try:
                bundle = ship_commit(remote, worktree, base, timings.parent) if ship else None
            except ShipError as error:
                note(log, str(error))
                return INFRASTRUCTURE_EXIT, []
            arguments = remote_gate_arguments(remote, sha, remote_ids, base, bundle, engine_test_args)
            with opened(log) as output:
                process = start_remote(remote.ssh(arguments), worktree, output)
                pump = process.pump
                try:
                    if not concurrent:
                        remote_code, process = wait_process(process), None
                    if not remote_code and local_ids:
                        local_code = local()
                    if process:
                        remote_code, process = wait_process(process), None
                finally:
                    if process:  # an interrupt: stop the host run while the log is still open
                        stop_process(process)
        elif local_ids:
            local_code = local()
    finally:
        if bundle:
            drop_bundle(remote, bundle)
    if pump and pump.windows_lint_skipped:
        note(log, f"WARNING: {remote.host} skipped the Windows lint (it lacks the target, or a C compiler for it); "
                  "engine code for Windows is not linted in this run")
    steps = []
    if remote_ids and remote_code not in (SSH_FAILED_EXIT, REMOTE_BUSY_EXIT, INFRASTRUCTURE_EXIT):
        steps = [{**step, "host": remote.host} for step in remote_steps_timings(remote)]
    try:
        steps += json.loads(local_timings.read_text())["latest"]["steps"]
    except (OSError, ValueError, KeyError):
        pass  # a gate that failed to start has no timings; the exit code says so
    return combine_codes(remote_exit(remote, remote_code, log), local_code), steps


def run_split_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
                   timings: Path, command: list[str], remote: Remote) -> int:
    """After the merge: the remote areas run on the host, then the rest here; the first failure
    ends the run. The commit's title and body go to the local half as the pull request event."""
    with commit_event_file(worktree) as event:
        code, steps = split_gate(worktree, base, only, log, timings, command, remote, event=event)
    if steps:
        write_json(timings, {"latest": {"steps": steps}})
    return code


def run_gate(worktree: Path, base: str | None, only: str | None, log: Path | None,
             timings: Path, command: list[str], remote: Remote | None = None) -> int:
    if remote is None:
        return run_local_gate(worktree, base, only, log, timings, command)
    return run_split_gate(worktree, base, only, log, timings, command, remote)


def step_with_prerequisites(step: str) -> str:
    """The `--only` selection that runs `step` after what it reads (`check_all.sh` keeps file order)."""
    return ",".join([*PREREQUISITES.get(step, ()), step])


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
