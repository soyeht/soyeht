#!/usr/bin/env python3
"""Run the Swift client's integration test against the engine programs the suites run.

The test (`nativeClientReconnectsAndDrivesItsOwnShellAndEditor` in `apps/shared`) starts the
supervisor and the engine and drives them through `EngineClient`. A change to the engine can break
it while every Rust test passes, and the full `swift test` that would say so runs only after the
merge. This runs that one test in the fast tier, with the programs of `e2e.stage_programs`: the
same stable copy the suites start, so when the suites run on this Mac the TCC guard around
them also watches this run.

Swift Testing exits 0 when a filter matches nothing, so the run passes only when the output says
the named test passed, exactly once.
"""
import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

import e2e

TEST_NAME = "nativeClientReconnectsAndDrivesItsOwnShellAndEditor"
PACKAGE = e2e.ROOT / "apps/shared"
BIN_DIR_ENV = "SOYEHT_TEST_BIN_DIR"


def passed_once(output: str, name: str = TEST_NAME) -> bool:
    """Whether Swift Testing reports `name` passed exactly once and no test failed."""
    passes = re.findall(rf"(?m)^.*Test {re.escape(name)}\(\) passed after ", output)
    failures = re.findall(r"(?m)^.*Test \w+\(\) failed after ", output)
    return len(passes) == 1 and not failures


def swift_command(name: str = TEST_NAME) -> list[str]:
    return ["swift", "test", "--package-path", str(PACKAGE), "--filter", name]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=e2e.ROOT / "engine/target/debug")
    args = parser.parse_args()
    binaries = args.bin_dir.resolve()
    e2e.require_binaries(binaries, e2e.PRODUCT_NAMES, "build workspace binaries before this test")
    programs = e2e.stage_programs(binaries) if e2e.MACOS else binaries
    environment = {**os.environ, BIN_DIR_ENV: str(programs)}
    lines = []
    with subprocess.Popen(swift_command(), env=environment, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True) as run:
        for line in run.stdout:
            sys.stdout.write(line)
            lines.append(line)
    if run.returncode:
        return run.returncode
    if not passed_once("".join(lines)):
        print(f"e2e_swift_client: {TEST_NAME} did not run exactly once", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
