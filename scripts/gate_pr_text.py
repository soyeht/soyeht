"""The lines of a pull request body that a gate reads: `Label: <subject> because <reason>`.

The pre-merge gate gets the pull request as an event file (`--event-file`); after the merge
`post_merge_gate.commit_event` builds the same event from the squash commit message, so the
checks that read these lines see the same text on both sides of the merge.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

# A reason says something: "none" or "n/a" after the word is not one.
MINIMUM_REASON_WORDS = 3
STATEMENT = re.compile(r"(?P<subject>\S.*?)\s+because\s+(?P<reason>\S.*)")


def body_lines(event_file: Path | None) -> list[str]:
    """The lines of the pull request body in `event_file`; none without one."""
    if event_file is None:
        return []
    event = json.loads(event_file.read_text())
    return (event.get("pull_request", {}).get("body") or "").splitlines()


def statements(lines: list[str], label: str) -> tuple[list[tuple[str, str]], list[str]]:
    """(subject, reason) of each `<label>: <subject> because <reason>` line, and the other
    lines that start with the label (they name nothing, or give no reason, and waive nothing)."""
    found: list[tuple[str, str]] = []
    unread: list[str] = []
    prefix = re.compile(rf"{re.escape(label)}:\s*(.*)")
    for line in lines:
        start = prefix.fullmatch(line.strip())
        if start is None:
            continue
        parsed = STATEMENT.fullmatch(start[1])
        if parsed and len(parsed["reason"].split()) >= MINIMUM_REASON_WORDS:
            found.append((parsed["subject"], parsed["reason"]))
        else:
            unread.append(line.strip())
    return found, unread
