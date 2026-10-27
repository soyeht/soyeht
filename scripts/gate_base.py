"""Resolve the commit that the local ratchets compare against."""

from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path


BASE_BRANCH = "origin/main"
# The directory the post-merge scripts write (`latest.json`, `watch.json`, `flakes.json`) and the
# fast plan reads. The operator sets it once for all of them; unset, it is `.cache/main-health` of
# the main checkout.
MAIN_HEALTH_DIR_ENV = "SOYEHT_MAIN_HEALTH_DIR"


class BaseError(ValueError):
    """The comparison base cannot be resolved; a ratchet must not guess one."""


def resolve_base(root: Path, explicit: str | None = None) -> str:
    """Return the explicit full OID, or the merge-base of origin/main and HEAD."""
    if explicit:
        if not re.fullmatch(r"[0-9a-f]{40}", explicit):
            raise BaseError("--base must be a full commit OID")
        return explicit
    result = subprocess.run(
        ["git", "merge-base", BASE_BRANCH, "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    base = result.stdout.strip()
    if result.returncode or not re.fullmatch(r"[0-9a-f]{40}", base):
        detail = result.stderr.strip() or "no common ancestor"
        raise BaseError(
            f"cannot resolve the merge-base of {BASE_BRANCH} and HEAD ({detail}); "
            "run `git fetch origin main` or pass --base FULL_COMMIT_OID"
        )
    return base


def main_checkout(root: Path) -> Path:
    """The main working tree, whichever worktree `root` is."""
    result = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=root, capture_output=True, text=True, check=True,
    )
    return Path(result.stdout.strip()).parent


def main_health_dir(root: Path) -> Path:
    """The one health directory: $SOYEHT_MAIN_HEALTH_DIR, else `.cache/main-health` of the main
    checkout. `post_merge.py`, `main_watch.py` and `gate_plan.py` all resolve it here, so the
    plan reads the directory the operator writes."""
    configured = os.environ.get(MAIN_HEALTH_DIR_ENV)
    if configured:
        return Path(configured).expanduser().resolve()
    return main_checkout(root) / ".cache/main-health"
