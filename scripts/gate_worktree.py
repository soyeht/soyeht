"""The checkouts the post-merge gate runs in (ADR 0089): the temporary worktree of a run, the
persistent one that keeps its cargo targets (`--worktree`), and the scratch directory.

Only a worktree this module created (it marks its git directory) is ever checked out forcibly and
cleaned: never the main checkout, the checkout the scripts run from, or anyone's work.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
from pathlib import Path

import check_editor_performance

WORKTREE_PREFIX = "soyeht-post-merge-"
# Where a run keeps its worktree. Unset, the system temporary directory; a test names its own so
# that two gates running the tests at once never remove each other's worktrees.
SCRATCH_ROOT_ENV = "SOYEHT_POST_MERGE_TMP"
EDITOR_REPORT = check_editor_performance.VM_REPORT.relative_to(check_editor_performance.ROOT)
# A persistent worktree keeps these between runs. The Swift `.build` directories, Xcode's derived
# data and the old reports of `.cache` are cleaned: SwiftPM's and Xcode's incremental builds miss
# cross-module changes across checkouts. The package checkouts and the VM1 report stay.
KEPT_BETWEEN_RUNS = ("engine/target", "apps/desktop/target", ".cache/apple-packages",
                     str(EDITOR_REPORT))
# Written in the git directory of a worktree this script created: only such a worktree is ever
# checked out forcibly and cleaned.
OWNED_MARKER = "soyeht-post-merge-owned"


class InfrastructureError(RuntimeError):
    """The machine or the gate could not run; nothing is recorded about main."""


def git(root: Path, *arguments: str, check: bool = True) -> str:
    result = subprocess.run(["git", *arguments], cwd=root, capture_output=True, text=True)
    if check and result.returncode:
        raise RuntimeError(f"git {' '.join(arguments)}: {result.stderr.strip()}")
    return result.stdout.strip()


def scratch_root() -> Path:
    return Path(os.environ.get(SCRATCH_ROOT_ENV) or tempfile.gettempdir())


def remove_stale_worktrees(root: Path, keep: Path | None = None) -> None:
    """Leftovers of a run that was killed; the lock says no other run is alive. The persistent
    worktree (`keep`) is not one, wherever it is."""
    git(root, "worktree", "prune", check=False)
    for stale in scratch_root().glob(f"{WORKTREE_PREFIX}*"):
        if keep and (stale.resolve() == keep or stale.resolve() in keep.parents):
            continue
        shutil.rmtree(stale, ignore_errors=True)
    git(root, "worktree", "prune", check=False)


def clean_worktree(worktree: Path) -> None:
    """Drop everything the last run left except the build directories (`KEPT_BETWEEN_RUNS`)."""
    keep = [argument for path in KEPT_BETWEEN_RUNS for argument in ("-e", path)]
    git(worktree, "clean", "-fdxq", *keep)


def own_checkout() -> Path:
    """The checkout this script runs from."""
    return Path(__file__).resolve().parents[1]


def owned_git_dir(root: Path, worktree: Path) -> Path:
    """The git directory of a worktree that this script created, or an InfrastructureError.

    Read from the `.git` file, so that it works when git itself fails in the worktree."""
    pointer = worktree / ".git"
    try:
        target = pointer.read_text().strip() if pointer.is_file() else ""
    except OSError:
        target = ""
    if not target.startswith("gitdir:"):
        raise InfrastructureError(f"{worktree} is not a linked worktree")
    git_dir = Path(target.removeprefix("gitdir:").strip()).resolve()
    common = Path(git(root, "rev-parse", "--path-format=absolute", "--git-common-dir")).resolve()
    if git_dir.parent != common / "worktrees":
        raise InfrastructureError(f"{worktree} is not a linked worktree of this repository")
    if not (git_dir / OWNED_MARKER).is_file():
        raise InfrastructureError(
            f"{worktree} was not created by post_merge.py; refusing to check out and clean it")
    return git_dir


def create_persistent_worktree(root: Path, worktree: Path, sha: str) -> None:
    if worktree.exists() and any(worktree.iterdir()):
        raise InfrastructureError(f"{worktree} exists and is not a worktree; refusing to use it")
    worktree.parent.mkdir(parents=True, exist_ok=True)
    git(root, "worktree", "add", "--detach", str(worktree), sha)
    git_dir = Path(git(worktree, "rev-parse", "--path-format=absolute", "--git-dir"))
    (git_dir / OWNED_MARKER).write_text("created by scripts/post_merge.py; checked out and cleaned there\n")


def prepare_persistent_worktree(root: Path, worktree: Path, sha: str) -> None:
    """Reuse `worktree` for `sha`, or create it.

    Only a worktree this script created is reused (the marker): the force and the clean below
    must never touch the main checkout, the checkout this script runs from, or anyone's work. A
    worktree that fails to prepare (a killed git left its index locked, say) is recreated."""
    resolved = worktree.resolve()
    if root.resolve() in (resolved, *resolved.parents):
        raise InfrastructureError(f"{worktree} is the main checkout or inside it")
    own = own_checkout()
    if resolved in (own, *own.parents):
        raise InfrastructureError(f"{worktree} contains the checkout this script runs from")
    try:
        if (worktree / ".git").exists():
            git_dir = owned_git_dir(root, worktree)
            (git_dir / "index.lock").unlink(missing_ok=True)  # the run lock is held: no git is running
            try:
                git(worktree, "checkout", "--quiet", "--detach", "--force", sha)
                clean_worktree(worktree)
            except RuntimeError as error:
                print(f"post_merge: recreating the worktree {worktree}: {error}", flush=True)
                git(root, "worktree", "remove", "--force", str(worktree), check=False)
                shutil.rmtree(worktree, ignore_errors=True)
                git(root, "worktree", "prune")
                create_persistent_worktree(root, worktree, sha)
        else:
            create_persistent_worktree(root, worktree, sha)
    except (RuntimeError, OSError) as error:
        raise InfrastructureError(f"cannot prepare the worktree {worktree}: {error}") from None
