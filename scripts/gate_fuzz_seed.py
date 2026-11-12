#!/usr/bin/env python3
"""The seed of the long fuzz run (`security:fuzz-long`, docs/security-testing.md).

    gate_fuzz_seed.py [COMMIT]     prints the seed for COMMIT (default HEAD)

A seed is a number taken from the commit id, so a run is the same for the same commit. The post-merge
gate hands the seed of the main commit it checks to every run of the step it makes (the first, the
rerun, the control on the last green main, and each probe of the culprit search over older commits),
so a crash a run found is replayed by the search instead of being lost to a fresh random seed, which
would call the run flaky or blame a commit that did not cause it. `SOYEHT_FUZZ_SEED` in the
environment wins over this.
"""

from __future__ import annotations

import re
import subprocess
import sys

# 15 hexadecimal digits: 60 bits, which the harness reads as a decimal `u64`.
SEED_HEX_DIGITS = 15
COMMIT_ID = re.compile(r"[0-9a-f]{40}")


def seed_for(commit: str) -> int:
    if not COMMIT_ID.fullmatch(commit):
        raise ValueError(f"not a full commit id: {commit!r}")
    return int(commit[:SEED_HEX_DIGITS], 16)


def main(arguments: list[str]) -> int:
    if len(arguments) > 1:
        print("usage: gate_fuzz_seed.py [COMMIT]", file=sys.stderr)
        return 64
    commit = arguments[0] if arguments else subprocess.run(
        ["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
    try:
        print(seed_for(commit))
    except ValueError as error:
        print(f"gate_fuzz_seed: {error}", file=sys.stderr)
        return 64
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
