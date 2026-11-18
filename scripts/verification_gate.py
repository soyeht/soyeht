"""The verification ratchet: acceptances, gate steps and ignored tests may not weaken unless a
`Verification change:` line of the pull request names the thing.

The number of tests is not a rule (deleting a test needs no reason); the name of each test the
head no longer has is printed so that a reviewer sees it. `hardening_gate.py` measures base and
head and calls `check_verification`.
"""

from __future__ import annotations

import re
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

from gate_pr_text import body_lines, statements

if TYPE_CHECKING:
    from hardening_gate import Metrics


VERIFICATION_LABEL = "Verification change"
WAIVED_THING = re.compile(r"(ignores|removes step|removes acceptance)\s+`?([^`]+?)`?")


@dataclass(frozen=True)
class Waivers:
    """What the `Verification change:` lines of a pull request waive, and nothing else."""

    ignored_tests: int = 0
    steps: frozenset[str] = frozenset()
    acceptances: frozenset[str] = frozenset()


def waivers(event_file: Path | None) -> Waivers:
    """One line per thing: `Verification change: ignores <test> because <reason>`,
    `removes step <id> because <reason>` or `removes acceptance <name> because <reason>`.
    A line that names nothing or gives no reason ("none") waives nothing, and is reported."""
    found, unread = statements(body_lines(event_file), VERIFICATION_LABEL)
    ignored, steps, acceptances = 0, set(), set()
    for subject, _ in found:
        thing = WAIVED_THING.fullmatch(subject)
        if thing is None:
            unread.append(f"{VERIFICATION_LABEL}: {subject}")
        elif thing[1] == "ignores":
            ignored += 1
        else:
            (steps if thing[1] == "removes step" else acceptances).add(thing[2])
    for line in unread:
        print(f"{VERIFICATION_LABEL}: waives nothing: {line!r}", file=sys.stderr)
    return Waivers(ignored, frozenset(steps), frozenset(acceptances))


def weakened_steps(base: "Metrics", head: "Metrics") -> list[tuple[str, str]]:
    """(id, what happened) of each step the head lost, moved out of the fast tier or out of `always`."""
    now = {step_id: (area, tier) for area, tier, step_id in head.steps}
    lost = []
    for area, tier, step_id in base.steps:
        if step_id not in now:
            lost.append((step_id, "was removed"))
        elif tier == "both" and now[step_id][1] != "both":
            lost.append((step_id, "left the fast tier"))
        elif area == "always" and now[step_id][0] != "always":
            lost.append((step_id, "left the `always` area"))
    return lost


def check_verification(base: "Metrics", head: "Metrics", waived: Waivers) -> bool:
    """Acceptances, gate steps and ignored tests may not drop or grow unless a line names them.

    The number of tests is not a rule: deleting a test needs no reason. The name of each test the
    head no longer has is printed, so a reviewer sees a deletion the diff may bury.
    """
    prefix = "check-verification-not-weakened:"
    failed = False
    print(f"{prefix} tests base={len(base.tests)} head={len(head.tests)} (not a rule)")
    for name in sorted((Counter(base.tests) - Counter(head.tests)).elements()):
        print(f"{prefix} removed test {name}")
    print(f"{prefix} acceptances base={len(base.acceptances)} head={len(head.acceptances)}")
    for name in sorted((Counter(base.acceptances) - Counter(head.acceptances)).elements()):
        covered = name in waived.acceptances
        print(f"{prefix} acceptance {name} was removed" + (" (waived)" if covered else ""),
              file=sys.stderr if not covered else sys.stdout)
        failed |= not covered
    print(f"{prefix} gate steps base={len(base.steps)} head={len(head.steps)}")
    for step_id, what in weakened_steps(base, head):
        covered = step_id in waived.steps
        print(f"{prefix} step {step_id} {what}" + (" (waived)" if covered else ""),
              file=sys.stderr if not covered else sys.stdout)
        failed |= not covered
    increase = head.ignored_tests - base.ignored_tests
    print(f"{prefix} ignored tests base={base.ignored_tests} head={head.ignored_tests}")
    if increase > waived.ignored_tests:
        print(f"{prefix} {increase} more ignored tests, {waived.ignored_tests} named by a line", file=sys.stderr)
        failed = True
    if failed:
        print(f"{prefix} add one `{VERIFICATION_LABEL}: removes step <id> because <reason>` line per "
              "step, `removes acceptance <name> because <reason>` per acceptance and "
              "`ignores <test> because <reason>` per ignored test to the pull request body "
              "(a reason is at least three words)", file=sys.stderr)
    return failed
