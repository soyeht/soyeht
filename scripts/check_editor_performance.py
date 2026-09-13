#!/usr/bin/env python3
"""Measure the slice-four editor speed budgets on the reference Mac."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache"
RUST_REPORT = CACHE / "editor-performance-rust.json"
MAC_REPORT = CACHE / "editor-performance-mac.json"
REPORT = CACHE / "editor-performance.json"
LIMITS = {
    "open_100k_ms": 150.0,
    "keystroke_to_pixel_p95_ms": 16.0,
    "scroll_frame_p95_ms": 1000.0 / 60.0,
    "search_1gb_ms": 1000.0,
}
MINIMUMS = {"editor_render_ink_pixels": 500}


def run() -> dict[str, float | int]:
    CACHE.mkdir(exist_ok=True)
    subprocess.run(
        ["cargo", "run", "--locked", "--release", "--quiet", "-p", "soyeht-editor",
         "--example", "editor_performance", "--", "--report", str(RUST_REPORT)],
        cwd=ROOT / "engine", check=True,
    )
    executable = (
        CACHE / "mac-derived/Build/Products/Debug/Soyeht Dev.app/Contents/MacOS/Soyeht Dev"
    )
    if not executable.is_file():
        raise RuntimeError("build the Mac app before the editor performance gate")
    subprocess.run(
        [str(executable), "--editor-performance-report", str(MAC_REPORT)],
        cwd=ROOT, check=True,
    )
    measurements = json.loads(RUST_REPORT.read_text()) | json.loads(MAC_REPORT.read_text())
    REPORT.write_text(json.dumps(measurements, indent=2, sort_keys=True) + "\n")
    return measurements


def failures(measurements: dict[str, float | int]) -> list[str]:
    failed = [
        name for name, limit in LIMITS.items()
        if float(measurements[name]) >= limit
    ]
    if int(measurements["scroll_frames"]) < 60:
        failed.append("scroll_frames")
    failed.extend(
        name for name, minimum in MINIMUMS.items()
        if int(measurements[name]) < minimum
    )
    return failed


def main() -> int:
    try:
        measurements = run()
        for name, limit in LIMITS.items():
            value = float(measurements[name])
            print(f"editor-performance: {name}={value:.3f} limit={limit:.3f}")
        for name, minimum in MINIMUMS.items():
            value = int(measurements[name])
            print(f"editor-performance: {name}={value} minimum={minimum}")
        failed = failures(measurements)
        if failed:
            print("editor-performance: failed " + ", ".join(failed), file=sys.stderr)
            return 1
        print("editor-performance: passed")
        return 0
    except (OSError, RuntimeError, KeyError, ValueError, json.JSONDecodeError,
            subprocess.CalledProcessError) as error:
        print(f"editor-performance: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
