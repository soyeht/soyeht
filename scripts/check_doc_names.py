#!/usr/bin/env python3
"""Fail when a document names a wire method, event or setting that does not exist.

`docs/` and `contracts/` Markdown names methods and events in backticks (`pane.list`). A name
that no longer exists, or never did, sends a reader looking for something that is not
there. The check reads every dotted name in inline code, and every double-quoted dotted
name inside a fenced block, whose first segment is a protocol namespace (`pane`, `task`,
`engine`, ...), and accepts it when it is:

- a method or event of `contracts/protocol.json`, or a key of its `x-settings` (a `<name>`
  segment stands for any text);
- a string literal in production or test code, which is how the structured log events
  (`engine.ready`) are defined;
- a field path of the schema: each segment is a property of the object the segment before
  it holds (`agent.canSignOut`), or a method or event followed by fields of its parameters
  or result (`pane.auth_finished.reason`);
- a file name, which is a name whose last segment is a known extension (`report.json`).

Anything else fails. Fix the document: use the real name, or remove the reference.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = "contracts/protocol.json"
DOC_ROOTS = ("docs", "contracts")
# Dated reports record what was true when they were written, so they may name what is gone.
SKIPPED_DOCS = ("docs/reports/",)
# Names a document may keep although the protocol has no such name, with the reason.
HISTORICAL_NAMES = {
    "pane.attention": "left column of the name mapping table in contract 08 section 9",
}
# `scripts/` is left out: its tests invent names, and a name must be real in the product.
CODE_ROOTS = ("engine", "apps", "contracts")
CODE_SUFFIXES = (".rs", ".swift", ".py", ".ts")
SKIPPED_PARTS = {"target", ".build", "node_modules", "DerivedData"}
FILE_EXTENSIONS = {
    "json", "jsonl", "sock", "lock", "log", "vscdb", "md", "rs", "swift", "toml", "sql", "py",
    "sh", "ts", "plist", "txt", "yml", "yaml", "db", "sqlite", "pen", "png",
}  # fmt: skip
NAME = r"[a-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_<>]+)+"
# A name opens the code span; arguments such as `pane.list {paneId}` may follow it.
INLINE = re.compile(rf"`({NAME})(?=[`\s{{(|,:;])")
QUOTED = re.compile(rf'"({NAME})"')
CODE_LITERAL = re.compile(r'"([a-z][A-Za-z0-9_]*(?:\.[a-z][A-Za-z0-9_]*)+)"')
FENCE = re.compile(r"^\s*(```|~~~)")


class Vocabulary:
    """What a dotted name in a document may legitimately be."""

    def __init__(self, root: Path) -> None:
        self.schema = json.loads((root / SCHEMA).read_text(encoding="utf-8"))
        self.methods = set(self.schema["x-methods"])
        self.events = set(self.schema["x-events"])
        self.settings = set(self.schema.get("x-settings", {}))
        self.wire = self.methods | self.events
        self.children: dict[str, set[str]] = {}
        self.collect_children(self.schema)
        self.code = code_literals(root)
        self.namespaces = {
            name.split(".")[0] for name in self.wire | self.settings if "." in name
        }
        self.templates = [
            re.compile("^" + r"[^.]+".join(re.escape(part) for part in re.split(r"<[^>]*>", name)) + "$")
            for name in self.settings
            if "<" in name
        ]

    def properties_of(self, node: object) -> dict[str, object]:
        """Return the properties an object schema declares, following references."""
        if not isinstance(node, dict):
            return {}
        if "$ref" in node:
            return self.properties_of(self.schema["$defs"].get(node["$ref"].rsplit("/", 1)[-1]))
        found = dict(node.get("properties", {}))
        for key in ("items", "anyOf", "oneOf", "allOf"):
            value = node.get(key)
            for part in value if isinstance(value, list) else [value]:
                found.update(self.properties_of(part))
        return found

    def collect_children(self, node: object) -> None:
        """Map each property name to the property names of the objects it can hold."""
        if isinstance(node, dict):
            for name, value in node.get("properties", {}).items():
                self.children.setdefault(name, set()).update(self.properties_of(value))
            for value in node.values():
                self.collect_children(value)
        elif isinstance(node, list):
            for value in node:
                self.collect_children(value)

    def field_path(self, segments: list[str]) -> bool:
        """True when each segment is a property of the object that the one before it holds."""
        return segments[0] in self.children and all(
            after in self.children.get(before, ())
            for before, after in zip(segments, segments[1:])
        )

    def payload_path(self, segments: list[str]) -> bool:
        """True for a method or event followed by fields of its parameters or result."""
        for count in range(1, len(segments)):
            entry = {**self.schema["x-methods"], **self.schema["x-events"]}.get(".".join(segments[:count]))
            if entry is None:
                continue
            roots = [self.properties_of(entry.get(key)) for key in ("params", "result")]
            for fields in roots:
                node: object = {"properties": fields}
                for segment in segments[count:]:
                    fields = self.properties_of(node)
                    if segment not in fields:
                        break
                    node = fields[segment]
                else:
                    return True
        return False

    def known(self, name: str) -> bool:
        if name in self.wire or name in self.settings or name in self.code or name in HISTORICAL_NAMES:
            return True
        if any(template.match(name) for template in self.templates):
            return True
        segments = name.split(".")
        while len(segments) > 1 and segments[-1].isdigit():
            segments = segments[:-1]  # A rotated file: `engine.log.1`.
        if segments[-1] in FILE_EXTENSIONS:
            return True
        return self.field_path(segments) or self.payload_path(segments)


def code_literals(root: Path) -> set[str]:
    found: set[str] = set()
    for base in CODE_ROOTS:
        for path in (root / base).rglob("*"):
            if path.suffix in CODE_SUFFIXES and not SKIPPED_PARTS.intersection(path.parts):
                found.update(CODE_LITERAL.findall(path.read_text(encoding="utf-8", errors="replace")))
    return found


def document_names(text: str) -> list[tuple[int, str]]:
    """Return (line, name) for every dotted name in inline code or a fenced block's strings."""
    found: list[tuple[int, str]] = []
    fenced = False
    for number, line in enumerate(text.splitlines(), 1):
        if FENCE.match(line):
            fenced = not fenced
            continue
        pattern = QUOTED if fenced else INLINE
        found += [(number, match.group(1)) for match in pattern.finditer(line)]
    return found


def problems(root: Path) -> list[str]:
    vocabulary = Vocabulary(root)
    found: list[str] = []
    mentioned: set[str] = set()
    for base in DOC_ROOTS:
        for path in sorted((root / base).rglob("*.md")):
            relative = path.relative_to(root).as_posix()
            if relative.startswith(SKIPPED_DOCS):
                continue
            text = path.read_text(encoding="utf-8")
            for number, name in document_names(text):
                mentioned.add(name)
                if name.split(".")[0] in vocabulary.namespaces and not vocabulary.known(name):
                    found.append(f"{relative}:{number}: `{name}` is not a method, event, setting, field or log event")
    found += [
        f"HISTORICAL_NAMES lists {name} ({reason}), which no document names; remove it"
        for name, reason in HISTORICAL_NAMES.items()
        if name not in mentioned
    ]
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    found = problems(parser.parse_args().root.resolve())
    for line in found:
        print(line)
    if found:
        print(f"{len(found)} document names that do not exist")
    return 1 if found else 0


if __name__ == "__main__":
    raise SystemExit(main())
