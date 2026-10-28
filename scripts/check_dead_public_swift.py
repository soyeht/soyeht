#!/usr/bin/env python3
"""Fail on `public` declarations of `apps/shared` that nothing in the Swift code refers to.

`public` hides a declaration from the compiler's unused-code check, so a function that only
its own declaration names stays in the shared package for ever. The check lists every `public`
`func`, `var`, `let`, `struct`, `class`, `enum`, `actor`, `protocol` and `typealias` of
`apps/shared/Sources` whose name appears nowhere else in the Swift code of the repository
(`apps/shared`, `apps/dictation`, `apps/mac`, `apps/ios`, their tests, and the generated
`contracts/swift` sources), comments left out. The match is by name, so a name shared with
another declaration, a protocol requirement or a member of the standard library counts as
used: the check misses some dead code. It can also report a declaration that a framework
calls without naming it; the names in `CALLED_BY_FRAMEWORK` and the reviewed entries of
`scripts/dead_public_swift_allowlist.json` cover those.

Delete the declaration (and the tests that only exercise it), make it `internal` when only the
package itself uses it, or, when the framework reaches it in a way the name does not show,
add `"path::name": "reason"` to the allowlist. An allowlist entry that no longer matches a
candidate fails the check, so the list cannot go stale.
"""

from __future__ import annotations

import re
from collections import Counter
from pathlib import Path

from dead_declarations import problems_of, run

ALLOWLIST = Path(__file__).resolve().with_name("dead_public_swift_allowlist.json")
UNLISTED = (
    "{path}:{line}: public {name} is never referred to by the Swift code; delete it "
    "(and the tests that only use it), make it internal, or allowlist it with a reason"
)
DEFINITION_ROOT = "apps/shared/Sources"
REFERENCE_ROOTS = ("apps", "contracts/swift/Sources")
IGNORED_PARTS = {".build", "build", "DerivedData", "node_modules"}
DECLARATION = re.compile(r"\b(func|var|let|struct|class|enum|actor|protocol|typealias|case)\s+(\w+)")
PUBLIC_DECLARATION = re.compile(
    r"^[ \t]*(?:@[\w.]+(?:\([^)\n]*\))?[ \t]+)*public[ \t]+"
    r"(?:(?:final|static|class|override|nonisolated|mutating|indirect|convenience|required|"
    r"unowned|weak|lazy|dynamic|distributed)[ \t]+)*"
    r"(?:func|var|let|struct|class|enum|actor|protocol|typealias)[ \t]+(\w+)",
    re.M,
)
IDENTIFIER = re.compile(r"[A-Za-z_]\w*")
STRING_START = re.compile(r'(#*)("""|")')
# The framework calls these by name: a SwiftUI view's `body`, the `Codable`, `Hashable` and
# `LocalizedError` witnesses, the representable and style hooks, and `callAsFunction`.
CALLED_BY_FRAMEWORK = {
    "body",
    "callAsFunction",
    "debugDescription",
    "defaultValue",
    "description",
    "dismantleNSView",
    "dismantleUIView",
    "encode",
    "errorDescription",
    "hash",
    "makeBody",
    "makeCoordinator",
    "makeNSView",
    "makeNSViewController",
    "makeUIView",
    "makeUIViewController",
    "reduce",
    "updateNSView",
    "updateNSViewController",
    "updateUIView",
    "updateUIViewController",
}


def swift_files(root: Path, relative: str) -> list[Path]:
    return sorted(
        path
        for path in (root / relative).rglob("*.swift")
        if path.is_file() and not IGNORED_PARTS.intersection(path.relative_to(root).parts)
    )


def blank_comments(source: str) -> str:
    """Replace the comments of Swift source with blanks, keeping its line numbers.

    String literals are skipped whole, so a `//` or a `/*` inside one is text, not a comment.
    Block comments nest, as they do in Swift.
    """
    out: list[str] = []
    i, n = 0, len(source)
    while i < n:
        if source.startswith("//", i):
            end = source.find("\n", i)
            end = n if end < 0 else end
            out.append(" " * (end - i))
            i = end
        elif source.startswith("/*", i):
            depth, end = 1, i + 2
            while end < n and depth:
                if source.startswith("/*", end):
                    depth, end = depth + 1, end + 2
                elif source.startswith("*/", end):
                    depth, end = depth - 1, end + 2
                else:
                    end += 1
            out.append(re.sub(r"[^\n]", " ", source[i:end]))
            i = end
        elif opening := STRING_START.match(source, i):
            hashes, quotes = opening.group(1), opening.group(2)
            end = string_end(source, opening.end(), hashes, quotes)
            out.append(source[i:end])
            i = end
        else:
            out.append(source[i])
            i += 1
    return "".join(out)


def string_end(source: str, start: int, hashes: str, quotes: str) -> int:
    """The offset after the string literal whose body begins at `start`."""
    closing = quotes + hashes
    escape = "\\" + hashes
    i, n = start, len(source)
    while i < n:
        if source.startswith(escape, i):
            i += len(escape) + 1
        elif source.startswith(closing, i):
            return i + len(closing)
        elif len(quotes) == 1 and source[i] == "\n":
            return i  # An unterminated single-line literal ends with its line.
        else:
            i += 1
    return n


def code_of(path: Path) -> str:
    """The source with its comments blanked and its line numbers kept."""
    return blank_comments(path.read_text(encoding="utf-8"))


def referenced_names(root: Path) -> Counter[str]:
    """Count each identifier that is not the name a declaration introduces."""
    counts: Counter[str] = Counter()
    seen: set[Path] = set()
    for relative in REFERENCE_ROOTS:
        for path in swift_files(root, relative):
            if path in seen:
                continue
            seen.add(path)
            code = code_of(path)
            declared = {match.start(2) for match in DECLARATION.finditer(code)}
            for match in IDENTIFIER.finditer(code):
                if match.start() not in declared:
                    counts[match.group(0)] += 1
    return counts


def candidates(root: Path) -> list[tuple[str, int, str]]:
    """Return (path, line, name) for every `public` declaration that nothing refers to."""
    counts = referenced_names(root)
    found: list[tuple[str, int, str]] = []
    for path in swift_files(root, DEFINITION_ROOT):
        code = code_of(path)
        relative = path.relative_to(root).as_posix()
        for match in PUBLIC_DECLARATION.finditer(code):
            name = match.group(1)
            if counts[name] == 0 and name not in CALLED_BY_FRAMEWORK:
                found.append((relative, code.count("\n", 0, match.start(1)) + 1, name))
    return found


problems = problems_of(candidates, UNLISTED, "dead public declaration")


if __name__ == "__main__":
    raise SystemExit(run(__doc__, ALLOWLIST, problems))
