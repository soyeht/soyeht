#!/usr/bin/env python3
"""Run the Swift client's integration tests against the engine programs the suites run.

The tests (in `apps/shared`) start the supervisor and the engine and drive them through
`EngineClient`: `nativeClientReconnectsAndDrivesItsOwnShellAndEditor` is the client's whole
surface, `phonePairsByConsentReadsFreshStateAndLosesRevokedAccess` is the paired device's
(pairing by consent, fresh state, revoked access), and
`manyLongTerminalsConnectOnceAndKeepTheirHistory` is seven terminals of long output in two
workspaces, the connection a Mac app with that many terminals makes (one connection, no
slow-consumer close, every terminal's history, and again after a reconnect), and
`theClientKeepsItsConnectionAndItsKeysMovingWhileTheMainActorIsBusy` is the transport under a busy
main actor (a request answered, the connection kept and the keys typed sent while the main actor is
held, ADR 0171). A change to the
engine's stream, screen or outbox code, or to the client's terminal or connection code, can break
them while every unit test passes, and the full `swift test` that would say so runs only after the
merge. This runs them in the fast tier, with the programs of `e2e.stage_programs`: the same stable
copy the suites start, so when the suites run on this Mac the TCC guard around them also watches
this run.

Swift Testing exits 0 when a filter matches nothing, so the run passes only when the output says
each named test passed, exactly once.
"""
import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

import e2e

TEST_NAMES = (
    "nativeClientReconnectsAndDrivesItsOwnShellAndEditor",
    "manyLongTerminalsConnectOnceAndKeepTheirHistory",
    "phonePairsByConsentReadsFreshStateAndLosesRevokedAccess",
    "theClientKeepsItsConnectionAndItsKeysMovingWhileTheMainActorIsBusy",
)
# What the tests start: the product programs, the engine with the test hooks and the fake agent
# (`PhoneConnectionTests` starts both).
PROGRAMS = (*e2e.PRODUCT_NAMES, e2e.TEST_ENGINE, "soyeht-fake-acp")
PACKAGE = e2e.ROOT / "apps/shared"
BIN_DIR_ENV = "SOYEHT_TEST_BIN_DIR"


def passed_once(output: str, name: str) -> bool:
    """Whether Swift Testing reports `name` passed exactly once and no test failed."""
    passes = re.findall(rf"(?m)^.*Test {re.escape(name)}\(\) passed after ", output)
    failures = re.findall(r"(?m)^.*Test \w+\(\) failed after ", output)
    return len(passes) == 1 and not failures


def swift_command(name: str) -> list[str]:
    return ["swift", "test", "--package-path", str(PACKAGE), "--filter", name]


def run_test(name: str, environment: dict[str, str]) -> int:
    """One test in its own `swift test`: Swift Testing runs the tests of a run at the same time,
    and the many-terminals one keeps the main actor busy, so alone each has the host to itself."""
    lines = []
    with subprocess.Popen(swift_command(name), env=environment, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True) as run:
        for line in run.stdout:
            sys.stdout.write(line)
            lines.append(line)
    if run.returncode:
        return run.returncode
    if not passed_once("".join(lines), name):
        print(f"e2e_swift_client: {name} did not run exactly once", file=sys.stderr)
        return 1
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=e2e.ROOT / "engine/target/debug")
    args = parser.parse_args()
    binaries = args.bin_dir.resolve()
    e2e.require_binaries(binaries, PROGRAMS, "build the engine programs (the workspace and the test engine) before this test")
    staged = tuple(map(e2e.executable, PROGRAMS))
    programs = e2e.stage_programs(binaries, staged) if e2e.MACOS else binaries
    environment = {**os.environ, BIN_DIR_ENV: str(programs)}
    # Every test runs, so one failure does not hide the next, and any non-zero code fails the step:
    # a `swift test` killed by a signal returns a negative code that a `max` would read as a pass.
    codes = [run_test(name, environment) for name in TEST_NAMES]
    return 1 if any(codes) else 0


if __name__ == "__main__":
    raise SystemExit(main())
