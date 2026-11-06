"""Test support for the count ratchets: a fixture engine tree and a run of `main` against it."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from collections.abc import Callable
from pathlib import Path

ENGINE = "engine/soyeht-engine"


def fixture_root(case: unittest.TestCase, lib: str, module: str, snippet: str) -> Path:
    """An engine tree: `lib` as the crate root, `module` a test-only file, `snippet` elsewhere.

    The snippet also sits in the engine's `tests/` and in another crate, which no ratchet counts.
    """
    directory = tempfile.TemporaryDirectory()
    case.addCleanup(directory.cleanup)
    root = Path(directory.name)
    for relative in (f"{ENGINE}/src", f"{ENGINE}/tests", "engine/soyeht-store/src"):
        (root / relative).mkdir(parents=True)
    (root / f"{ENGINE}/src/lib.rs").write_text(lib)
    (root / f"{ENGINE}/src/fixtures.rs").write_text(module)
    (root / f"{ENGINE}/tests/it.rs").write_text(snippet)
    (root / "engine/soyeht-store/src/lib.rs").write_text(snippet)
    return root


def run_main(
    case: unittest.TestCase, main: Callable[[], int], root: Path, key: str, allowed: int
) -> int:
    """The exit code of `main` for a tree whose baseline allows `allowed` sites."""
    baseline = root / "baseline.json"
    baseline.write_text(json.dumps({key: allowed}))
    arguments = sys.argv
    sys.argv = ["check", "--root", str(root), "--baseline", str(baseline)]
    case.addCleanup(setattr, sys, "argv", arguments)
    return main()
