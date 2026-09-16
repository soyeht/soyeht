#!/usr/bin/env python3
"""Echo Cargo output and require one named filtered test to have run."""

from __future__ import annotations

import re
import sys


def validates(output: str, expected: str) -> bool:
    named = re.findall(rf"(?m)^test {re.escape(expected)} \.\.\.", output)
    summaries = re.findall(
        r"(?m)^test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; "
        r"\d+ filtered out;",
        output,
    )
    return len(named) == 1 and len(summaries) == 1


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: require_one_rust_test.py TEST_NAME", file=sys.stderr)
        return 2
    output = sys.stdin.read()
    sys.stdout.write(output)
    if validates(output, sys.argv[1]):
        return 0
    print(
        f"isolated Rust test did not run exactly once: {sys.argv[1]}",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
