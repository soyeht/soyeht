#!/usr/bin/env python3
"""The checks an agent runs before it pushes: the failures the merge gate kept catching.

    uv run python scripts/agent_precheck.py [--quick] [--pr-text FILE]

No build, no restricted vocabulary needed (when FORBIDDEN_TERMS is set, it is used). In order:

1. `git fetch origin main`: the ratchets compare with the merge-base and fail without it.
2. The cheap `always` checks of the gate. The list is read from `check_all.sh --list --tier
   fast` and each step runs through `check_all.sh --only ID`, so the commands live in one place
   and a new `always` check joins the precheck by being added to the gate. A `:unit` step (the
   tests of a gate script) runs only when the script it tests, or the test, changed.
3. The commit messages of `BASE..HEAD` (and `--pr-text FILE`, the title and body you are about
   to give `gh pr create`) for AI attribution; with the vocabulary, the full scan too.
4. `swift format lint --strict` on the changed `.swift` files (line length is 100).
5. `contracts/generate.py --check` and the contracts unit tests, which prove every design
   token against the commit generated before the Mac used them (a new Mac token goes in
   `ADDED_AFTER_GENERATION` of `contracts/test_design_tokens.py`).
6. `check_quality.py`, last: its duplication scan takes minutes. `--quick` leaves it out.

Every failing check is reported, not only the first. Exit code 1 when any failed, 2 when the
precheck itself could not run. The merge gate is `check_all.sh --tier fast`; this is not a
substitute for it, only what to run first.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import time
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

from gate_base import BaseError, resolve_base

ROOT = Path(__file__).resolve().parents[1]
CHECK_ALL = ROOT / "scripts" / "check_all.sh"
STEP_LINE = re.compile(r"^(\w+)\s+(both|full|fast)\s+(\S+)\s*$")
# `step always both ID uv run python -m unittest discover -s scripts -p 'test_x.py'`
# The unit tests of the gate scripts are the `gate_scripts` area, listed when a script changed.
CHEAP_AREAS = ("always", "gate_scripts")
UNIT_STEP = re.compile(r"^\s*step\s+(?:always|gate_scripts)\s+\S+\s+(\S+)\s+.*-p\s+'(test_\w+\.py)'",
                       re.MULTILINE)
IMPORT = re.compile(r"^\s*(?:import|from)\s+(\w+)", re.MULTILINE)
# The directories the gate's `swift:format-lint` step lints (Package.swift and the generated
# Swift are not among them and fail `--strict`).
FORMAT_STEP = re.compile(r"^\s*step\s+swift\s+\S+\s+swift:format-lint\s+swift format lint --strict "
                         r"--recursive (.+)$", re.MULTILINE)
# Unit steps that take minutes: they run only when the script they test, or a module that
# script imports (at any depth), changed. The other unit steps take seconds and all run when
# any gate tooling changed.
SLOW_UNITS = ("post-merge:unit", "main-watch:unit")
GATE_TOOLING = ("scripts/", ".githooks/", ".github/")
FETCH_TIMEOUT_SECONDS = 60
# The checks that are not run as cheap steps: the precheck does them itself, or last.
OWN_STEPS = ("forbidden-terms:scan",)
QUALITY_STEP = "quality:check"
TIMINGS = ROOT / ".cache" / "agent-precheck-timings.json"
FAILURE_TAIL_LINES = 40


@dataclass
class Result:
    name: str
    ok: bool
    seconds: float
    output: str = ""
    rerun: str = ""  # the command that runs this check alone


def run(arguments: list[str], env: dict | None = None,
        timeout: float | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(arguments, cwd=ROOT, capture_output=True, text=True,
                          env={**os.environ, **(env or {})}, timeout=timeout)


def git(*arguments: str) -> str:
    result = run(["git", *arguments])
    if result.returncode:
        raise RuntimeError(f"git {' '.join(arguments)}: {result.stderr.strip()}")
    return result.stdout


def fetch_main() -> str | None:
    """Fetch origin/main; the warning when that was not possible (an old base still works)."""
    try:
        result = run(["git", "fetch", "origin", "main"], timeout=FETCH_TIMEOUT_SECONDS)
        problem = result.stderr.strip() if result.returncode else None
    except subprocess.TimeoutExpired:
        problem = f"timed out after {FETCH_TIMEOUT_SECONDS}s"
    if problem is None:
        return None
    if run(["git", "rev-parse", "--verify", "--quiet", "origin/main"]).returncode:
        raise RuntimeError(f"cannot fetch origin/main and none is known: {problem}")
    return "could not fetch origin/main; comparing with the one fetched earlier"


def changed_files(base: str) -> list[str]:
    """Paths changed since `base`: commits, staged and unstaged work, and untracked files."""
    tracked = git("diff", "--name-only", "--no-renames", base).splitlines()
    untracked = git("ls-files", "--others", "--exclude-standard").splitlines()
    return sorted(set(tracked) | set(untracked))


def listed_steps(base: str) -> list[tuple[str, str]]:
    """(area, id) of the gate's steps for the fast tier, from `check_all.sh --list`."""
    result = run(["bash", str(CHECK_ALL), "--list", "--tier", "fast", "--base", base])
    if result.returncode:
        raise RuntimeError(f"check_all.sh --list failed: {result.stderr.strip()}")
    return [(m[1], m[3]) for line in result.stdout.splitlines() if (m := STEP_LINE.match(line))]


def unit_tests(check_all_text: str) -> dict[str, str]:
    """step id -> test file, for every `always`/`gate_scripts` step that runs one test file."""
    return {step_id: test for step_id, test in UNIT_STEP.findall(check_all_text)}


def import_closure(test: str, read=lambda path: path.read_text()) -> set[str]:
    """File names of `scripts/<test>` and of every script module it imports, at any depth."""
    found, pending = set(), [test]
    while pending:
        name = pending.pop()
        if name in found:
            continue
        try:
            text = read(ROOT / "scripts" / name)
        except OSError:
            continue  # not a module of scripts/ (a library), or a test that cannot be read
        found.add(name)
        pending.extend(f"{module}.py" for module in IMPORT.findall(text))
    return found


def affected_by(test: str, changed: list[str], read=lambda path: path.read_text()) -> bool:
    """Whether a change reaches the gate script tested by `scripts/<test>`.

    The test file, a script module it imports at any depth, or a file it names (check_all.sh,
    a baseline) counts."""
    names = {Path(path).name for path in changed if path.startswith(GATE_TOOLING[:2])}
    closure = import_closure(test, read)
    if not closure:
        return True  # a test that cannot be read cannot be ruled out
    try:
        text = read(ROOT / "scripts" / test)
    except OSError:
        return True
    return bool(names & closure) or any(name in text for name in names if not name.endswith(".py"))


def select_steps(steps: list[tuple[str, str]], units: dict[str, str], changed: list[str],
                 read=lambda path: path.read_text()) -> tuple[list[str], list[str], bool]:
    """(cheap step ids, contracts step ids, whether quality:check is listed), in gate order."""
    cheap, contracts, quality = [], [], False
    for area, step_id in steps:
        if area not in CHEAP_AREAS or step_id in OWN_STEPS:
            continue
        if step_id == QUALITY_STEP:
            quality = True
        elif step_id.startswith("contracts:"):
            contracts.append(step_id)
        elif step_id in units:
            if step_id in SLOW_UNITS:
                wanted = affected_by(units[step_id], changed, read)
            else:
                wanted = any(path.startswith(GATE_TOOLING) for path in changed)
            if wanted:
                cheap.append(step_id)
        else:
            cheap.append(step_id)
    return cheap, contracts, quality


def timed(name: str, arguments: list[str], env: dict | None = None, rerun: str = "") -> Result:
    started = time.monotonic()
    result = run(arguments, env)
    return Result(name, result.returncode == 0, time.monotonic() - started,
                  result.stdout + result.stderr, rerun or " ".join(arguments))


def gate_step(step_id: str) -> Result:
    # check_all.sh asks for the vocabulary on any run; the steps chosen here never read it.
    env = {"SOYEHT_GATE_TIMINGS": str(TIMINGS)}
    if not os.environ.get("FORBIDDEN_TERMS"):
        env["SOYEHT_SKIP_FORBIDDEN_TERMS"] = "1"
    return timed(step_id, ["bash", str(CHECK_ALL), "--only", step_id], env,
                 rerun=f"bash scripts/check_all.sh --only {step_id}")


def message_checks(base: str, pr_text: list[Path]) -> list[Result]:
    scanner = [sys.executable, str(ROOT / "scripts" / "forbidden_terms.py")]
    texts = [argument for path in pr_text for argument in ("--text-file", str(path))]
    results = [timed("forbidden-terms:messages",
                     [*scanner, "--messages-only", "--base", base, *texts])]
    if os.environ.get("FORBIDDEN_TERMS", "").strip():
        results.append(timed("forbidden-terms:scan", [*scanner, "--base", base, *texts]))
    return results


def format_roots(check_all_text: str) -> list[str]:
    """The directories the gate's `swift:format-lint` step lints, read from check_all.sh."""
    found = FORMAT_STEP.search(check_all_text)
    return found[1].split() if found else []


def swift_format(changed: list[str], roots: list[str]) -> list[Result]:
    files = [path for path in changed
             if path.endswith(".swift") and path.startswith(tuple(f"{root}/" for root in roots))
             and (ROOT / path).is_file()]
    if not files:
        return []
    if shutil.which("swift") is None:
        return [Result("swift-format", True, 0.0, "skipped: no `swift` on PATH")]
    return [timed("swift-format", ["swift", "format", "lint", "--strict", *files])]


def report(result: Result) -> None:
    mark = "ok  " if result.ok else "FAIL"
    print(f"precheck: {mark} {result.name} ({result.seconds:.1f}s)", flush=True)
    if not result.ok:
        lines = result.output.rstrip().splitlines()
        for line in lines[-FAILURE_TAIL_LINES:]:
            print(f"    {line}")
        if result.rerun:
            print(f"    rerun: {result.rerun}")
    elif result.output.startswith("skipped"):
        print(f"    {result.output}")


def checks(cheap: list[str], contracts: list[str], with_quality: bool, base: str,
           changed: list[str], pr_text: list[Path], roots: list[str]) -> Iterator[Result]:
    """The checks in order, each run when the caller asks for it, so its report comes at once."""
    for step_id in cheap:
        yield gate_step(step_id)
    yield from message_checks(base, pr_text)
    yield from swift_format(changed, roots)
    for step_id in contracts:
        yield gate_step(step_id)
    if with_quality:
        yield gate_step(QUALITY_STEP)


def main(argv: list[str] | None = None) -> int:
    """Exit 0 when every check passed, 1 when one failed, 2 when the precheck could not run."""
    try:
        return run_precheck(argv)
    except (RuntimeError, BaseError) as error:
        print(f"precheck: {error}", file=sys.stderr)
    except Exception as error:  # noqa: BLE001 - an unexpected failure is not a failed check
        print(f"precheck: unexpected {type(error).__name__}: {error}", file=sys.stderr)
    return 2


def run_precheck(argv: list[str] | None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    parser.add_argument("--quick", action="store_true",
                        help="leave out check_quality.py (its duplication scan takes minutes)")
    parser.add_argument("--pr-text", type=Path, action="append", default=[], metavar="FILE",
                        help="the title and body you will give `gh pr create`, scanned for attribution")
    arguments = parser.parse_args(argv)
    warning = fetch_main()
    base = resolve_base(ROOT)
    changed = changed_files(base)
    gate_text = CHECK_ALL.read_text()
    steps = listed_steps(base)
    if not any(area == "always" for area, _ in steps):
        raise RuntimeError("no `always` step parsed from `check_all.sh --list`; has its format changed?")
    roots = format_roots(gate_text)
    if not roots:
        raise RuntimeError("no directories parsed from the swift:format-lint step of check_all.sh")
    cheap, contracts, quality = select_steps(steps, unit_tests(gate_text), changed)
    if warning:
        print(f"precheck: warning: {warning}")
    print(f"precheck: base {base[:12]}, {len(changed)} changed files", flush=True)
    results: list[Result] = []
    for outcome in checks(cheap, contracts, quality and not arguments.quick, base, changed,
                          arguments.pr_text, roots):
        report(outcome)
        results.append(outcome)
    failed = [result.name for result in results if not result.ok]
    if failed:
        print(f"precheck: FAILED: {', '.join(failed)}")
        return 1
    print("precheck: passed" + (" (check_quality skipped)" if arguments.quick else "")
          + "; the merge gate is still `check_all.sh --tier fast`")
    return 0


if __name__ == "__main__":
    sys.exit(main())
