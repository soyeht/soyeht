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
PREFIX = "check-verification-not-weakened:"
# A step runs in the fast tier when its tier is `both` or `fast`, in the full tier for `both` or `full`.
FAST_TIERS = ("both", "fast")
FULL_TIERS = ("both", "full")


@dataclass(frozen=True)
class Waivers:
    """What the `Verification change:` lines of a pull request waive, and nothing else."""

    ignored_tests: frozenset[str] = frozenset()
    steps: frozenset[str] = frozenset()
    acceptances: frozenset[str] = frozenset()


def waivers(event_file: Path | None) -> Waivers:
    """One line per thing: `Verification change: ignores <test> because <reason>`,
    `removes step <id> because <reason>` or `removes acceptance <name> because <reason>`.
    A line that names nothing or gives no reason ("none") waives nothing, and is reported."""
    found, unread = statements(body_lines(event_file), VERIFICATION_LABEL)
    named: dict[str, set[str]] = {"ignores": set(), "removes step": set(), "removes acceptance": set()}
    for subject, _ in found:
        thing = WAIVED_THING.fullmatch(subject)
        if thing is None:
            unread.append(f"{VERIFICATION_LABEL}: {subject}")
        else:
            named[thing[1]].add(thing[2])
    for line in unread:
        print(f"{VERIFICATION_LABEL}: waives nothing: {line!r}", file=sys.stderr)
    return Waivers(*(frozenset(named[key]) for key in ("ignores", "removes step", "removes acceptance")))


def weakened_steps(base: "Metrics", head: "Metrics") -> list[tuple[str, str]]:
    """(id, what happened) of each step the head lost, no longer runs in a tier it ran in, or
    moved to an area other than `always`."""
    now = {step_id: (area, tier) for area, tier, step_id, _ in head.steps}
    lost = []
    for area, tier, step_id, _ in base.steps:
        if step_id not in now:
            lost.append((step_id, "was removed"))
            continue
        moved_area, moved_tier = now[step_id]
        if tier in FAST_TIERS and moved_tier not in FAST_TIERS:
            lost.append((step_id, "left the fast tier"))
        elif tier in FULL_TIERS and moved_tier not in FULL_TIERS:
            lost.append((step_id, "left the full tier"))
        elif moved_area != area and moved_area != "always":
            lost.append((step_id, f"moved from area {area} to {moved_area}"))
    return lost


def refused(what: str, covered: bool) -> bool:
    """Print a weakening, on stderr unless a line names it; whether it fails the check."""
    print(f"{PREFIX} {what}" + (" (waived)" if covered else ""), file=sys.stdout if covered else sys.stderr)
    return not covered


def check_verification(base: "Metrics", head: "Metrics", waived: Waivers) -> bool:
    """Acceptances, gate steps and ignored tests may not drop or grow unless a line names them.

    The number of tests is not a rule: deleting a test needs no reason. The name of each test the
    head no longer has is printed, so a reviewer sees a deletion the diff may bury.
    """
    failed = False
    print(f"{PREFIX} tests base={len(base.tests)} head={len(head.tests)} (not a rule)")
    for name in sorted((Counter(base.tests) - Counter(head.tests)).elements()):
        print(f"{PREFIX} removed test {name}")
    print(f"{PREFIX} acceptances base={len(base.acceptances)} head={len(head.acceptances)}")
    for name in sorted((Counter(base.acceptances) - Counter(head.acceptances)).elements()):
        failed |= refused(f"acceptance {name} was removed", name in waived.acceptances)
    print(f"{PREFIX} gate steps base={len(base.steps)} head={len(head.steps)}")
    for step_id, what in weakened_steps(base, head):
        failed |= refused(f"step {step_id} {what}", step_id in waived.steps)
    commands = {step_id: command for _, _, step_id, command in base.steps}
    for _, _, step_id, command in head.steps:
        if step_id in commands and commands[step_id] != command:
            print(f"{PREFIX} step {step_id} changed its command")
    print(f"{PREFIX} ignored tests base={len(base.ignored_tests)} head={len(head.ignored_tests)}")
    for name in sorted(set(head.ignored_tests) - set(base.ignored_tests)):
        failed |= refused(f"test {name} is ignored", bool({name, name.rpartition("::")[2]} & waived.ignored_tests))
    if failed:
        print(f"{PREFIX} add one `{VERIFICATION_LABEL}: removes step <id> because <reason>` line per "
              "step, `removes acceptance <name> because <reason>` per acceptance and "
              "`ignores <test> because <reason>` per newly ignored Rust test (the function name, or "
              "path::name) to the pull request body (a reason is at least three words)", file=sys.stderr)
    return failed


def revert_lines(base: "Metrics", head: "Metrics", reason: str) -> list[str]:
    """The lines that waive what `check_verification` would refuse in `head`, for a pull request
    that restores an earlier state and cannot ask its author (`main_watch.py`)."""
    lines = [f"{VERIFICATION_LABEL}: removes acceptance {name} because {reason}"
             for name in sorted(set(base.acceptances) - set(head.acceptances))]
    lines += [f"{VERIFICATION_LABEL}: removes step {step_id} because {reason}"
              for step_id, _ in weakened_steps(base, head)]
    lines += [f"{VERIFICATION_LABEL}: ignores {name} because {reason}"
              for name in sorted(set(head.ignored_tests) - set(base.ignored_tests))]
    return lines
