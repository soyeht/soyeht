"""What a post-merge run records in the health directory (ADR 0089): the JSON written for a
commit, the earlier run that a `--force` rerun keeps in `history/`, the facts about a commit that
go into a record, and the report printed at the end.
"""

from __future__ import annotations

import json
import shutil
import time
from pathlib import Path

from gate_worktree import git
from post_merge_editor import NEEDS_MEASUREMENT

HISTORY_DIRECTORY = "history"


def commit_info(root: Path, sha: str) -> dict:
    subject, author = git(root, "log", "-1", "--format=%s%x00%an", sha).split("\0")
    return {"sha": sha, "subject": subject, "author": author}


def parent_of(root: Path, sha: str) -> str | None:
    parents = git(root, "rev-list", "--parents", "-n", "1", sha).split()
    return parents[1] if len(parents) > 1 else None


def write_json(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")


def archive_previous_run(health: Path, sha: str) -> dict | None:
    """Keep the run a `--force` rerun replaces, and say what it was.

    The first, failing, run is the only evidence of a flake. Its log and timings move to
    `<health>/history/<sha>.<started_at>.*`, and its record is copied there (the record stays
    in place until the new one replaces it, so a rerun that cannot start loses nothing).
    Returns a summary for the `previous` field of the new record, or None when nothing was run."""
    names = {"log": f"{sha}.log", "json": f"{sha}.json", "timings.json": f"{sha}.timings.json"}
    sources = [health / name for name in names.values() if (health / name).exists()]
    if not sources:
        return None
    previous = read_record(health / names["json"])
    started = previous.get("started_at") or time.strftime(
        "%Y-%m-%dT%H:%M:%SZ", time.gmtime(max(path.stat().st_mtime for path in sources)))
    history = health / HISTORY_DIRECTORY
    history.mkdir(parents=True, exist_ok=True)
    # A rerun that died before it wrote its record leaves this `started_at` in place: the next
    # archive must not replace the evidence of the first one.
    base_stem = f"{sha}.{started.replace(':', '')}"
    stem, attempt = base_stem, 1
    while any((history / f"{stem}.{suffix}").exists() for suffix in names):
        stem, attempt = f"{base_stem}-{attempt}", attempt + 1
    for suffix, name in names.items():
        source = health / name
        if not source.exists():
            continue
        if suffix == "json":
            shutil.copyfile(source, history / f"{stem}.{suffix}")
        else:
            source.replace(history / f"{stem}.{suffix}")
    summary = {key: previous[key] for key in ("result", "exit_code", "failed_step", "culprit")
               if key in previous}
    return {**summary, "started_at": started, "log": str(history / f"{stem}.log")}


def report(record: dict) -> str:
    if record["result"] == "pass":
        return f"post_merge: main {record['sha'][:12]} is green ({record['duration_seconds']:.0f}s)"
    if record["result"] == NEEDS_MEASUREMENT:
        return (f"post_merge: main {record['sha'][:12]} needs a VM1 editor measurement "
                f"(editor tree {record['editor_tree_hash']}); main is not red\n  note: {record['note']}\n"
                f"  log: {record['log']}")
    state = "FLAKY (not green)" if record["result"] == "flaky" else "RED"
    lines = [f"post_merge: main {record['sha'][:12]} is {state} (step {record['failed_step']})"]
    culprit = record.get("culprit")
    if culprit:
        lines.append(f"  culprit: {culprit['sha'][:12]} {culprit['subject']} ({culprit['author']})")
    for candidate in record.get("candidates", []):
        if not culprit:
            lines.append(f"  candidate: {candidate['sha'][:12]} {candidate['subject']}")
    if record.get("note"):
        lines.append(f"  note: {record['note']}")
    lines.append(f"  log: {record['log']}")
    lines.append("  Fix forward or revert the culprit within the hour; main is not touched.")
    return "\n".join(lines)


def read_record(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return {}
