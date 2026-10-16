#!/usr/bin/env python3
"""Run the full gate on new main commits and revert a confirmed culprit (ADR 0092).

    main_watch.py [--apply] [--repo DIR] [--health-dir DIR] [--remote HOST --remote-root PATH]

One pass: fetch `origin/main`; when it moved since the last pass recorded in
`<health-dir>/watch.json`, run `scripts/post_merge.py` once on the newest commit (a batch of
all the new ones). The gate and the culprit search are post_merge's: a red step is rerun
once, a pass is `flaky` and blames nobody, otherwise a search over the commits since the last
green main names one culprit, or several candidates when commits cannot run the failing step.

On a confirmed culprit, with `--apply`, it reverts that commit: a branch `main-watch/revert-<sha>`
from `origin/main` with `git revert`, pushed as a new branch, a pull request titled
"Revert <subject>" whose body names the failing step and the log, and `gh pr merge --squash`.
The author of the culprit re-lands a fixed change. It does not revert when the culprit is
ambiguous (candidates), when the run was flaky or the failing step is unknown, when the culprit
is itself a revert, when it was reverted already, or when the revert does not apply cleanly:
it records and reports instead.

`--dry-run` is the default: the gate runs and the plan is written, but nothing is pushed or
merged, and the pass is not marked as done, so a later `--apply` still acts on it. The script
only ever creates, pushes and deletes its own revert branches; it never rewrites history or
touches another branch.

`<health-dir>/watch.json` holds the last processed commit, the result and the revert. Exit
codes: 0 green or nothing new, 1 main was red (see the file), 2 the gate could not run,
3 a revert was needed and failed, 75 another run is in progress.
"""

from __future__ import annotations

import argparse
import fcntl
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path

import post_merge
from gate_base import main_checkout, main_health_dir

STATUS_FILE = "watch.json"
LOCK_FILE = "main_watch.lock"
BRANCH_PREFIX = "main-watch/revert-"
GREEN_EXIT, RED_EXIT, INFRASTRUCTURE_EXIT, REVERT_FAILED_EXIT = 0, 1, 2, 3
HISTORY_LIMIT = 50
LOG_TAIL_LINES = 40
LOG_TAIL_CHARS = 3000
# Used when the checkout has no git identity of its own.
FALLBACK_IDENTITY = ("-c", "user.name=main watch", "-c", "user.email=main-watch@users.noreply.github.com")


@dataclass
class Revert:
    culprit: str
    branch: str
    title: str
    # planned (dry run) | merged | open (pushed, not merged) | skipped | failed
    state: str
    pr: str | None = None
    reason: str | None = None


def read_json(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return {}


def write_json(path: Path, data: dict) -> None:
    post_merge.write_json(path, data)


def revert_title(subject: str) -> str:
    return f"Revert {subject}"


def log_excerpt(record: dict) -> str:
    """The end of the gate log, where the failing step prints its output."""
    try:
        lines = Path(record["log"]).read_text(errors="replace").splitlines()
    except (OSError, KeyError):
        return "(the log is not available)"
    return "\n".join(lines[-LOG_TAIL_LINES:])[-LOG_TAIL_CHARS:]


def revert_reason(record: dict, culprit: dict) -> str:
    return (
        f"This reverts commit {culprit['sha']}.\n\n"
        f"The full gate failed on main {record['sha'][:12]} at step {record['failed_step']}, and the "
        f"search over the commits since the last green main named {culprit['sha'][:12]} "
        f"as the first one that fails it. The step also failed when run again on the same "
        "commit, so it is not flaky."
    )


def revert_body(record: dict, culprit: dict) -> str:
    log = Path(record.get("log", "")).name or "unknown"
    return (
        f"{revert_reason(record, culprit)}\n\n"
        f"Failing step: `{record['failed_step']}`\n"
        f"Log: `{log}` in `.cache/main-health` of the machine that ran the gate. End of the log:\n\n"
        f"```\n{log_excerpt(record)}\n```\n\n"
        "The author re-lands a fixed change. This revert skips the fast gate: it restores a state "
        "that was green.\n"
    )


def identity(root: Path) -> list[str]:
    configured = post_merge.git(root, "config", "user.name", check=False)
    return [] if configured else list(FALLBACK_IDENTITY)


def is_revert_subject(subject: str) -> bool:
    return subject.startswith("Revert ")


def skip_reason(root: Path, culprit: dict) -> str | None:
    """Why this culprit must not be reverted automatically, or None."""
    sha = culprit["sha"]
    if is_revert_subject(culprit["subject"]):
        return "the culprit is a revert: reverting it would restore the failing change"
    on_main = subprocess.run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], cwd=root)
    if on_main.returncode:
        return "the culprit is not in the history of origin/main"
    reverted = post_merge.git(root, "log", "origin/main", "--format=%H", "-F",
                              f"--grep=This reverts commit {sha}", check=False)
    if reverted:
        return f"the culprit was reverted already by {reverted.splitlines()[0][:12]}"
    return None


def run_command(arguments: list[str], cwd: Path) -> str:
    result = subprocess.run(arguments, cwd=cwd, capture_output=True, text=True)
    if result.returncode:
        detail = (result.stderr or result.stdout).strip().splitlines()
        raise RuntimeError(f"{' '.join(arguments[:3])}: {detail[-1] if detail else 'failed'}")
    return result.stdout.strip()


def prepare_revert(root: Path, tree: Path, branch: str, culprit: dict, record: dict) -> None:
    """Commit the revert on a new branch from origin/main in `tree`."""
    post_merge.git(root, "worktree", "add", "-B", branch, str(tree), "origin/main")
    parents = post_merge.git(root, "rev-list", "--parents", "-n", "1", culprit["sha"]).split()
    options = ["-m", "1"] if len(parents) > 2 else []
    try:
        run_command(["git", "revert", "--no-commit", *options, culprit["sha"]], tree)
    except RuntimeError:
        subprocess.run(["git", "revert", "--abort"], cwd=tree, capture_output=True)
        raise RuntimeError("the revert does not apply cleanly to origin/main") from None
    message = f"{revert_title(culprit['subject'])}\n\n{revert_reason(record, culprit)}"
    run_command(["git", *identity(root), "commit", "--quiet", "-m", message], tree)


def land_revert(root: Path, branch: str, title: str, body: str, gh: list[str]) -> str:
    """Push the branch as a new branch (never forced), open the pull request, merge it."""
    run_command(["git", "push", "origin", f"{branch}:refs/heads/{branch}"], root)
    url = run_command([*gh, "pr", "create", "--base", "main", "--head", branch,
                       "--title", title, "--body", body], root).splitlines()[-1]
    try:
        run_command([*gh, "pr", "merge", url, "--squash"], root)
    except RuntimeError as error:
        raise MergeFailed(url, str(error)) from None
    return url


class MergeFailed(RuntimeError):
    def __init__(self, url: str, reason: str):
        super().__init__(reason)
        self.url = url


def revert_culprit(root: Path, record: dict, culprit: dict, apply: bool, gh: list[str],
                   scratch: Path) -> Revert:
    branch = BRANCH_PREFIX + culprit["sha"][:12]
    title = revert_title(culprit["subject"])
    reason = skip_reason(root, culprit)
    if reason:
        return Revert(culprit["sha"], branch, title, "skipped", reason=reason)
    if not apply:
        return Revert(culprit["sha"], branch, title, "planned", reason="dry run; pass --apply")
    tree = scratch / "revert-tree"
    try:
        prepare_revert(root, tree, branch, culprit, record)
        url = land_revert(root, branch, title, revert_body(record, culprit), gh)
    except MergeFailed as error:
        return Revert(culprit["sha"], branch, title, "open", pr=error.url, reason=str(error))
    except RuntimeError as error:
        return Revert(culprit["sha"], branch, title, "failed", reason=str(error))
    finally:
        post_merge.git(root, "worktree", "remove", "--force", str(tree), check=False)
        post_merge.git(root, "branch", "-D", branch, check=False)
    # Its own branch, merged: best effort, a leftover branch is harmless.
    subprocess.run(["git", "push", "origin", "--delete", branch], cwd=root, capture_output=True)
    return Revert(culprit["sha"], branch, title, "merged", pr=url)


def classify(record: dict) -> str:
    if record.get("result") == "pass":
        return "green"
    if record.get("result") == "flaky":
        return "flaky"
    if record.get("culprit"):
        return "culprit"
    if record.get("candidates"):
        return "ambiguous"
    return "unknown"


def decide(root: Path, record: dict, apply: bool, gh: list[str], scratch: Path) -> tuple[str, Revert | None]:
    """(result, revert) for a recorded run."""
    kind = classify(record)
    if kind == "culprit":
        revert = revert_culprit(root, record, record["culprit"], apply, gh, scratch)
        outcomes = {"merged": "red-reverted", "planned": "red-revert-planned", "open": "red-revert-open",
                    "skipped": "red-not-reverted", "failed": "red-revert-failed"}
        return outcomes[revert.state], revert
    return {"green": "green", "flaky": "flaky", "ambiguous": "red-ambiguous", "unknown": "red-unknown"}[kind], None


def new_commits(root: Path, last: str | None, head: str) -> list[str]:
    if not last:
        return [head]
    known = subprocess.run(["git", "cat-file", "-e", f"{last}^{{commit}}"], cwd=root, capture_output=True)
    if known.returncode:
        return [head]
    return post_merge.git(root, "rev-list", "--first-parent", "--reverse", f"{last}..{head}",
                          check=False).split()


def summary(result: str, head: str, record: dict, revert: Revert | None) -> str:
    lines = [f"main_watch: {result} at {head[:12]}"]
    if record.get("failed_step"):
        lines.append(f"  failing step: {record['failed_step']}")
    for commit in record.get("candidates", []):
        lines.append(f"  candidate: {commit['sha'][:12]} {commit['subject']}")
    if record.get("note"):
        lines.append(f"  note: {record['note']}")
    if revert:
        lines.append(f"  revert of {revert.culprit[:12]}: {revert.state}"
                     + (f" ({revert.reason})" if revert.reason else "")
                     + (f" {revert.pr}" if revert.pr else ""))
    return "\n".join(lines)


def run_pass(root: Path, health: Path, arguments: argparse.Namespace) -> int:
    status_path = health / STATUS_FILE
    status = read_json(status_path)
    gh = arguments.gh.split()
    post_merge.git(root, "fetch", "origin", "main")
    head = post_merge.git(root, "rev-parse", "origin/main")
    commits = new_commits(root, status.get("last_sha"), head)
    base = {**status, "head": head, "dry_run": not arguments.apply,
            "checked_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    if not commits:
        write_json(status_path, {**base, "result": "idle"})
        print(f"main_watch: nothing new on origin/main since {head[:12]}")
        return GREEN_EXIT
    argv = ["--repo", str(root), "--sha", head, "--health-dir", str(health),
            "--check-all", arguments.check_all]
    for option, value in (("--since", arguments.since), ("--remote", arguments.remote),
                          ("--remote-root", arguments.remote_root)):
        if value:
            argv += [option, value]
    code = post_merge.main(argv)
    if code == post_merge.BUSY_EXIT:
        return post_merge.BUSY_EXIT
    record = read_json(health / f"{head}.json")
    if code not in (0, 1) or not record:
        write_json(status_path, {**base, "result": "infrastructure", "note": "the gate could not run"})
        print("main_watch: the gate could not run; nothing was recorded or reverted", file=sys.stderr)
        return INFRASTRUCTURE_EXIT
    scratch = Path(tempfile.mkdtemp(prefix="soyeht-main-watch-", dir=post_merge.scratch_root()))
    try:
        result, revert = decide(root, record, arguments.apply, gh, scratch)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    history = (status.get("reverts", []) + ([asdict(revert)] if revert else []))[-HISTORY_LIMIT:]
    update = {
        **base, "result": result, "batch": len(commits), "failed_step": record.get("failed_step"),
        "log": record.get("log"), "culprit": record.get("culprit"),
        "candidates": record.get("candidates", []), "note": record.get("note"),
        "revert": asdict(revert) if revert else None, "reverts": history,
    }
    if arguments.apply:  # a dry run leaves the pass open for a later --apply
        update["last_sha"] = head
    write_json(status_path, update)
    print(summary(result, head, record, revert))
    if revert and revert.state in ("failed", "open"):
        return REVERT_FAILED_EXIT
    return GREEN_EXIT if result == "green" else RED_EXIT


def terminate(signal_number: int, frame) -> None:
    raise SystemExit(128 + signal_number)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--apply", action="store_true", help="push and merge the revert of a confirmed culprit")
    mode.add_argument("--dry-run", action="store_true", help="the default: change nothing outside .cache")
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--health-dir", type=Path)
    parser.add_argument("--since", help="last known green commit when none is recorded")
    parser.add_argument("--check-all", default="bash scripts/check_all.sh")
    parser.add_argument("--remote", metavar="HOST")
    parser.add_argument("--remote-root", metavar="PATH")
    parser.add_argument("--gh", default=os.environ.get("SOYEHT_GH", "gh"), help="the GitHub CLI command")
    arguments = parser.parse_args(argv)
    if bool(arguments.remote) != bool(arguments.remote_root):
        parser.error("--remote and --remote-root go together")
    root = main_checkout(arguments.repo)
    health = arguments.health_dir or main_health_dir(arguments.repo)
    health.mkdir(parents=True, exist_ok=True)
    with (health / LOCK_FILE).open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            print("main_watch: another run is in progress", file=sys.stderr)
            return post_merge.BUSY_EXIT
        signal.signal(signal.SIGTERM, terminate)
        return run_pass(root, health, arguments)


if __name__ == "__main__":
    sys.exit(main())
