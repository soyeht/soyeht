#!/usr/bin/env python3
"""Run the pre-merge gate with its Linux steps on the remote host (ADR 0089, docs/dev-local.md).

    gate_remote.py [--tier fast|full] [--only ID[,ID...]] [--base FULL_OID] [--event-file FILE]
                   [--remote HOST --remote-root PATH [--remote-slot NAME]] [--sequential]

It stands in for `bash scripts/check_all.sh --tier fast ...` on a busy Mac. Run it from the worktree
to gate; the script itself may belong to another checkout (a trusted main, as `main_watch.py` runs),
so a pull request that predates it is gated too. From the commit checked out in that worktree it takes
the steps `check_all.sh --list` says the tier runs (the plan: the areas the changed paths select, the
full tier when the gate scripts changed), sends those of the areas that run on Linux (`REMOTE_AREAS`:
engine, desktop, e2e, timing) to HOST through `scripts/remote_gate.sh`, as `post_merge.py --remote`
does after the merge, and runs the others (the `always` checks, Swift, TypeScript, the Android core)
here with `check_all.sh --only`. The result is one exit code: 0 when both halves pass, the failing
half's code otherwise, and 2 when the host could not be reached or could not run the gate (a clear
line in the output, never a pass).

The head of a pull request is on no remote the host reads, so it travels in a git bundle
(`post_merge_hosts.ship_commit`): `BASE..HEAD` when the host has the base, else the whole history;
`remote_gate.sh --bundle` fetches it and removes it. Both halves run at once, since the host's
work does not load this machine (`--sequential` runs the remote one first and stops at its
failure, as the run after the merge does). The engine tests of the fast tier cover the crates the
plan names (`engine:test-changed`), which the remote half receives as `--engine-test-args`; it runs
`engine:build` before any test of the engine. The per-step times of both halves, the remote ones
tagged with the host, go to `$SOYEHT_GATE_TIMINGS` (default `.cache/gate-timings.json` of the worktree).

HOST, PATH and NAME also come from SOYEHT_REMOTE_HOST, SOYEHT_REMOTE_ROOT and SOYEHT_REMOTE_SLOT.
The restricted vocabulary is never sent: the remote steps do not scan for it; the `always` half
here does. Without a host this command refuses to run; call `check_all.sh` for a local gate.
"""

from __future__ import annotations

import argparse
import os
import re
import shlex
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from gate_base import BaseError, resolve_base
from gate_timings import finalize
from gate_worktree import git, scratch_root
from post_merge_gate import INFRASTRUCTURE_EXIT, TO_STDOUT, split_gate
from post_merge_hosts import Remote

HOST_ENV, ROOT_ENV, SLOT_ENV = "SOYEHT_REMOTE_HOST", "SOYEHT_REMOTE_ROOT", "SOYEHT_REMOTE_SLOT"
SLOT_NAME = re.compile(r"[A-Za-z0-9_-]+")  # what remote_gate.sh --slot accepts
PLAN_ENGINE_TESTS = "GATE_ENGINE_TEST_ARGS="  # a line of `gate_plan.py --format shell`


def parse(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tier", choices=("fast", "full"), default="fast")
    parser.add_argument("--only", help="run only these steps (a rerun of the steps that failed)")
    parser.add_argument("--base", help="full commit OID (default: merge-base of origin/main and HEAD)")
    parser.add_argument("--event-file", type=Path, help="the pull request's title and body, for the scanners")
    parser.add_argument("--remote", metavar="HOST", default=os.environ.get(HOST_ENV),
                        help=f"the Linux host (${HOST_ENV})")
    parser.add_argument("--remote-root", metavar="PATH", default=os.environ.get(ROOT_ENV),
                        help=f"the checkout on the host that has scripts/remote_gate.sh (${ROOT_ENV})")
    parser.add_argument("--remote-slot", metavar="NAME", default=os.environ.get(SLOT_ENV) or "",
                        help=f"the host's worktree for this run, so that two gates run at once (${SLOT_ENV})")
    parser.add_argument("--sequential", action="store_true", help="run the remote half first and stop at its failure")
    parser.add_argument("--check-all", default="bash scripts/check_all.sh", help="the gate command run here")
    parser.add_argument("--plan", default="uv run python scripts/gate_plan.py", help="the command that prints the plan")
    arguments = parser.parse_args(argv)
    if not (arguments.remote and arguments.remote_root):
        parser.error(f"give --remote and --remote-root (or ${HOST_ENV} and ${ROOT_ENV})")
    if arguments.remote_slot and not SLOT_NAME.fullmatch(arguments.remote_slot):
        parser.error(f"--remote-slot must match {SLOT_NAME.pattern}")
    return arguments


def engine_test_args(arguments: argparse.Namespace, root: Path, base: str) -> str:
    """The engine crates `engine:test-changed` tests, as the commit's own fast plan selects them. A
    full tier or a `--only` run has no plan: the gate tests the whole workspace there."""
    if arguments.tier != "fast" or arguments.only:
        return ""
    result = subprocess.run([*arguments.plan.split(), "--tier", "fast", "--format", "shell", "--base", base],
                            cwd=root, capture_output=True, text=True)
    for token in (token for line in result.stdout.splitlines() for token in shlex.split(line)):
        if token.startswith(PLAN_ENGINE_TESTS):
            return token[len(PLAN_ENGINE_TESTS):]
    return ""  # no plan (the gate says why in its own run): the whole workspace


def run(arguments: argparse.Namespace, root: Path) -> int:
    # The host checks out HEAD, and the local half reads the working tree: they must be one tree.
    if git(root, "status", "--porcelain", "--untracked-files=no"):
        print("gate_remote: the worktree has uncommitted changes to tracked files; the host would gate "
              "the commit and this machine the working tree. Commit or discard them.", file=sys.stderr)
        return INFRASTRUCTURE_EXIT
    try:
        base = resolve_base(root, arguments.base)
    except BaseError as error:
        print(f"gate_remote: {error}", file=sys.stderr)
        return INFRASTRUCTURE_EXIT
    remote = Remote(arguments.remote, arguments.remote_root, arguments.remote_slot)
    started = time.time()
    with tempfile.TemporaryDirectory(dir=scratch_root()) as scratch:
        code, steps = split_gate(
            root, base, arguments.only, TO_STDOUT, Path(scratch) / "timings.json",
            arguments.check_all.split(), remote, tier=arguments.tier, event=arguments.event_file,
            ship=True, engine_test_args=engine_test_args(arguments, root, base),
            concurrent=not arguments.sequential)
    label = f"{arguments.tier}+remote:{remote.host}" + (f"+only:{arguments.only}" if arguments.only else "")
    if steps:
        finalize(steps, label, code, " ".join(sorted({step["area"] for step in steps})),
                 Path(os.environ.get("SOYEHT_GATE_TIMINGS") or root / ".cache/gate-timings.json"), started)
    print(report(git(root, "rev-parse", "HEAD"), remote, steps, code, time.time() - started), flush=True)
    return code


def report(sha: str, remote: Remote, steps: list[dict], code: int, seconds: float) -> str:
    """The split in one line: where the steps ran and for how long, and the result."""
    there = [step for step in steps if step.get("host")]
    here = [step for step in steps if not step.get("host")]
    split = (f"{len(there)} steps on {remote.host} ({sum(s['seconds'] for s in there):.0f}s), "
             f"{len(here)} here ({sum(s['seconds'] for s in here):.0f}s)")
    result = "passed" if code == 0 else f"FAILED (exit {code})"
    return f"gate_remote: {result} {sha[:12]}: {split}; {seconds:.0f}s wall"


def main(argv: list[str] | None = None) -> int:
    """The gate of the commit checked out in the current directory's worktree, by this script's code."""
    return run(parse(argv), Path(git(Path.cwd(), "rev-parse", "--show-toplevel")))


if __name__ == "__main__":
    sys.exit(main())
