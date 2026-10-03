"""Resolve the commit that the local ratchets compare against."""

from __future__ import annotations

import re
import subprocess
from pathlib import Path


BASE_BRANCH = "origin/main"


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
