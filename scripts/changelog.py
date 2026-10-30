#!/usr/bin/env python3
"""Draft, read and check the changelog (`CHANGELOG.md`, in the "Keep a Changelog" form).

    uv run python scripts/changelog.py draft [--version X.Y.Z] [--since REV] [--date YYYY-MM-DD] [--replace] [--print]
    uv run python scripts/changelog.py notes [X.Y.Z]     # the text of a version, as the release notes
    uv run python scripts/changelog.py check [X.Y.Z]     # the section exists and a person has edited it

`draft` lists the subjects of the squash commits that landed on the first-parent line since the
last tag (`v*`), or since `--since`, and writes them as a new section of `CHANGELOG.md` under
`[Unreleased]`, sorted into the categories by the verb the subject starts with. A subject ends
with the number of its pull request, which becomes a link. The section starts with a draft line:
the person edits the entries (a subject is written for a reviewer, a changelog for a user) and
deletes that line. `notes` and `check` refuse a section that still has it, so a draft can never be
published by accident. The version is `productVersion` of `contracts/release.json`.

With no tag yet (the first release) there is nothing to list: the section is a skeleton to write by
hand. Nothing here commits, tags or contacts the network.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

import release
from package_common import HOMEPAGE

ROOT = Path(__file__).resolve().parents[1]
CHANGELOG = ROOT / "CHANGELOG.md"
DRAFT_MARK = "<!-- draft:"
# The categories of keepachangelog.com, in the order they are written.
CATEGORIES = ("Added", "Changed", "Deprecated", "Removed", "Fixed", "Security")
# The first word of a subject decides its category; what no word names is a change.
VERBS = {
    "Added": ("add", "introduce", "tell", "show", "support", "package", "install"),
    "Fixed": ("fix", "repair", "retry", "try", "correct"),
    "Removed": ("remove", "delete", "drop"),
    "Deprecated": ("deprecate",),
    "Security": ("secure", "harden"),
}
SUBJECT = re.compile(r"^(?P<title>.*?)(?: \(#(?P<pr>[0-9]+)\))?$")
HEADING = re.compile(r"^## \[(?P<name>[^\]]+)\]", re.MULTILINE)
UNRELEASED = "Unreleased"
FIRST_RELEASE = (
    "First public release. Describe what a person can do with it, one entry per feature, and delete the "
    "other lines of this section."
)


class ChangelogError(Exception):
    """Something the person running the script can fix; the message says how."""


def git(*arguments, root=ROOT):
    result = subprocess.run(["git", *arguments], cwd=root, capture_output=True, text=True)
    if result.returncode:
        raise ChangelogError(f"git {' '.join(arguments)} failed: {result.stderr.strip()[-300:]}")
    return result.stdout


def last_tag(root=ROOT):
    """The newest tag of a release (`v` and a number), or None before the first one."""
    shown = subprocess.run(
        ["git", "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"], cwd=root, capture_output=True, text=True
    )
    return shown.stdout.strip() or None if shown.returncode == 0 else None


def squash_subjects(since, root=ROOT):
    """The subjects on the first-parent line of HEAD after `since`, newest first."""
    span = f"{since}..HEAD" if since else "HEAD"
    return [line for line in git("log", "--first-parent", "--format=%s", span, root=root).splitlines() if line.strip()]


def category_of(title):
    word = title.split(" ", 1)[0].lower()
    for category, verbs in VERBS.items():
        if word in verbs:
            return category
    return "Changed"


def entry(subject):
    """(category, line) of one subject: the title with its pull request as a link."""
    match = SUBJECT.match(subject)
    title = match["title"].strip().rstrip(".")
    link = f" ([#{match['pr']}]({HOMEPAGE}/pull/{match['pr']}))" if match["pr"] else ""
    return category_of(title), f"- {title}{link}."


def render(version, day, subjects, since):
    """The text of a new section: the draft line, then the entries by category."""
    lines = [f"## [{version}] - {day}", ""]
    if since is None and not subjects:
        lines += [f"{DRAFT_MARK} the first release has no earlier tag to list from. Write the entries, then delete this line. -->", "", "### Added", "", f"- {FIRST_RELEASE}", ""]
        return "\n".join(lines)
    note = f"{len(subjects)} merged pull request{'s' if len(subjects) != 1 else ''} since {since}"
    lines += [f"{DRAFT_MARK} generated from {note}. Edit the entries for a reader of the release, then delete this line. -->", ""]
    grouped = {category: [] for category in CATEGORIES}
    for subject in subjects:
        category, line = entry(subject)
        grouped[category].append(line)
    for category in CATEGORIES:
        if grouped[category]:
            lines += [f"### {category}", "", *grouped[category], ""]
    return "\n".join(lines)


def sections(text):
    """[(name, start, end)] of the `## [name]` sections of the changelog."""
    found = list(HEADING.finditer(text))
    return [(m["name"], m.start(), found[i + 1].start() if i + 1 < len(found) else len(text)) for i, m in enumerate(found)]


def section_of(text, version):
    for name, start, end in sections(text):
        if name == version:
            return text[start:end]
    return None


def insert(text, section, version, replace):
    """`text` with the section of `version` just below `[Unreleased]`."""
    existing = [(start, end) for name, start, end in sections(text) if name == version]
    if existing and not replace:
        raise ChangelogError(f"CHANGELOG.md already has a section for {version}; pass --replace to write it again")
    if existing:
        text = text[: existing[0][0]] + text[existing[0][1] :]
    block = section.rstrip("\n") + "\n"
    older = [start for name, start, _ in sections(text) if name != UNRELEASED]
    if older:
        return text[: older[0]] + block + "\n" + text[older[0] :]
    return text.rstrip("\n") + "\n\n" + block


def unreleased_has_entries(text):
    body = section_of(text, UNRELEASED) or ""
    return any(line.startswith("- ") for line in body.splitlines())


def notes_of(text, version):
    """The release notes of a version: its section without the heading; refuses a draft."""
    section = section_of(text, version)
    if section is None:
        raise ChangelogError(f"CHANGELOG.md has no section for {version}; run: uv run python scripts/changelog.py draft")
    if DRAFT_MARK in section:
        raise ChangelogError(
            f"the section for {version} in CHANGELOG.md is still a draft: edit its entries and delete the "
            f"line that starts with {DRAFT_MARK}"
        )
    body = section.split("\n", 1)[1].strip()
    if not any(line.startswith("- ") for line in body.splitlines()):
        raise ChangelogError(f"the section for {version} in CHANGELOG.md has no entry")
    return body + "\n"


def draft_command(arguments, root=ROOT, today=None):
    version = arguments.version or str(release.current(root).version)
    since = arguments.since or last_tag(root)
    subjects = squash_subjects(since, root) if since else []
    day = arguments.date or (today or datetime.now(timezone.utc).date()).isoformat()
    section = render(version, day, subjects, since)
    if arguments.print:
        print(section)
        return 0
    path = root / "CHANGELOG.md"
    text = path.read_text()
    path.write_text(insert(text, section, version, arguments.replace))
    print(f"changelog: wrote the draft of {version} ({len(subjects)} subject{"s" if len(subjects) != 1 else ""}) to CHANGELOG.md; edit it, then delete its draft line")
    if unreleased_has_entries(text):
        print("changelog: [Unreleased] has entries; move the ones that belong to this version into its section")
    return 0


def notes_command(arguments, root=ROOT):
    version = arguments.version or str(release.current(root).version)
    print(notes_of((root / "CHANGELOG.md").read_text(), version), end="")
    return 0


def check_command(arguments, root=ROOT):
    notes_command(arguments, root)
    return 0


def parser():
    top = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = top.add_subparsers(dest="command", required=True)
    drafted = commands.add_parser("draft", help="write the draft section of a version from the commits since the last tag")
    drafted.add_argument("--version", help="the version of the section (default: contracts/release.json)")
    drafted.add_argument("--since", help="list the commits after this revision (default: the last tag)")
    drafted.add_argument("--date", help="the date of the section, YYYY-MM-DD (default: today, UTC)")
    drafted.add_argument("--replace", action="store_true", help="write over an existing section of the version")
    drafted.add_argument("--print", action="store_true", help="print the section and write nothing")
    for name, help_text in (("notes", "print the release notes of a version"), ("check", "fail unless the section is written")):
        commands.add_parser(name, help=help_text).add_argument("version", nargs="?")
    return top


COMMANDS = {"draft": draft_command, "notes": notes_command, "check": check_command}


def main(argv=None):
    arguments = parser().parse_args(argv)
    try:
        return COMMANDS[arguments.command](arguments)
    except (ChangelogError, ValueError) as failure:
        print(f"changelog: {failure}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
