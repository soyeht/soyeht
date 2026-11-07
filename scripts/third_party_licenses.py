"""The license rules of the third-party notices (ADR 0149): the SPDX expression of a dependency,
which licenses a product may ship, which of the alternatives of an `OR` is the one Soyeht
follows, and which of a package's license files is that license's text.

Everything here is a pure function of its arguments, so `test_third_party_notices.py` covers it
without a build or a network."""

from __future__ import annotations

import itertools
import json
import re
from dataclasses import dataclass, field, replace
from pathlib import Path

DATA = Path(__file__).with_name("third_party")
POLICY = DATA / "policy.json"
SPDX_TEXTS = DATA / "spdx"

# What a license file's text says about its license, the most specific first. A file that none of
# these recognises is kept whatever license is elected: it is a custom or a combined notice.
SIGNATURES = (
    # The texts that mention the GNU licenses in passing come first (MPL-2.0 names the GPL, LGPL and
    # AGPL in its section 1.12; FreeType's license note names the GPL it is the alternative to).
    ("MPL-2.0", ("Mozilla Public License Version 2.0",)),
    ("FTL", ("FreeType Project LICENSE",)),
    ("FTL", ("FREETYPE LICENSES",)),
    ("LGPL-2.1-or-later", ("GNU LESSER GENERAL PUBLIC LICENSE", "Version 2.1, February 1999")),
    ("GPL-3.0-or-later", ("GNU GENERAL PUBLIC LICENSE", "Version 3")),
    ("GPL-2.0-only", ("GNU GENERAL PUBLIC LICENSE", "Version 2")),
    ("LGPL-3.0-or-later", ("GNU LESSER GENERAL PUBLIC LICENSE", "Version 3")),
    ("Apache-2.0", ("Apache License", "Version 2.0")),
    ("Unicode-3.0", ("UNICODE LICENSE V3",)),
    ("CC0-1.0", ("CC0 1.0 Universal",)),
    ("Unlicense", ("This is free and unencumbered software",)),
    ("BSL-1.0", ("Boost Software License",)),
    ("bzip2-1.0.6", ("bzip2", "libbzip2", "Julian R Seward")),
    ("Zlib", ("altered source versions must be plainly marked",)),
    ("ISC", ("Permission to use, copy, modify, and/or distribute this software for any purpose",)),
    ("BSD-3-Clause", ("Redistributions of source code must retain", "Neither the name")),
    ("BSD-2-Clause", ("Redistributions of source code must retain", "Redistributions in binary form")),
    ("MIT", ("Permission is hereby granted, free of charge",)),
)
NOTICE_PREFIXES = ("NOTICE", "ACKNOWLEDG")
# A file of one of these is source code or data, not a license, whatever its name (swift-syntax has a
# `CopyrightHeader.swift`).
SOURCE_SUFFIXES = (
    ".rs", ".toml", ".py", ".swift", ".c", ".h", ".cc", ".cpp", ".m", ".mm", ".js", ".ts", ".kt",
    ".java", ".go", ".rb", ".sh", ".cmake", ".in", ".yaml", ".yml", ".json", ".xml", ".html", ".lock",
)
# A file with one of these in its name is a collection of the licenses of bundled code: it is shipped
# whatever license is elected.
THIRD_PARTY_MARKS = ("THIRD-PARTY", "THIRDPARTY", "THIRD_PARTY")
LICENSE_PREFIXES = ("LICENSE", "LICENCE", "COPYING", "UNLICENSE", "COPYRIGHT", *NOTICE_PREFIXES)
SPDX_TOKEN = re.compile(r"\(|\)|[A-Za-z0-9][A-Za-z0-9.+\-]*")


class LicenseError(Exception):
    """An expression that cannot be read, or a license with no text to ship."""


@dataclass(frozen=True)
class Policy:
    allowed: frozenset
    prefer: tuple
    clarify: dict  # package name -> {"license": SPDX expression, "reason": ...}
    exceptions: dict  # (package name, license) -> reason
    bundled: dict = field(default_factory=dict)  # package name -> {"license", "files", "reason"}

    def rank(self, identifier):
        return self.prefer.index(identifier) if identifier in self.prefer else len(self.prefer)


def load_policy(path=POLICY):
    data = json.loads(Path(path).read_text())
    exceptions = {}
    for entry in data["exceptions"]:
        if not entry.get("reason", "").strip():
            raise LicenseError(f"the exception for {entry['package']} needs a reason")
        exceptions[(entry["package"], entry["license"])] = entry["reason"]
    for name, entry in data.get("bundled", {}).items():
        if not entry.get("reason", "").strip():
            raise LicenseError(f"the bundled entry for {name} needs a reason")
    return Policy(frozenset(data["allowed"]), tuple(data["prefer"]), data["clarify"], exceptions, data.get("bundled", {}))


# ---- SPDX expressions ------------------------------------------------------------------------


def parse(expression):
    """The tree of an SPDX expression: `("id", text)`, `("and", [..])` or `("or", [..])`. The
    `/` that old crates write for "or" is read as `OR`; `X WITH Y` is one identifier."""
    tokens = SPDX_TOKEN.findall(expression.replace("/", " OR "))
    if "".join(tokens) != re.sub(r"\s+", "", expression.replace("/", "OR")):
        raise LicenseError(f"cannot read the license expression {expression!r}")
    position = 0

    def peek():
        return tokens[position] if position < len(tokens) else None

    def take():
        nonlocal position
        if position >= len(tokens):
            raise LicenseError(f"the license expression {expression!r} ends too soon")
        position += 1
        return tokens[position - 1]

    def operands(operator, operand):
        found = [operand()]
        while peek() == operator:
            take()
            found.append(operand())
        return found[0] if len(found) == 1 else (operator.lower(), found)

    def term():
        token = take()
        if token == "(":
            inner = operands("OR", conjunction)
            if peek() != ")":
                raise LicenseError(f"unbalanced parenthesis in {expression!r}")
            take()
            return inner
        if token in ("AND", "OR", "WITH", ")"):
            raise LicenseError(f"unexpected {token} in {expression!r}")
        if peek() == "WITH":
            take()
            token = f"{token} WITH {take()}"
        return ("id", token)

    def conjunction():
        return operands("AND", term)

    tree = operands("OR", conjunction)
    if peek() is not None:
        raise LicenseError(f"unexpected {peek()} in {expression!r}")
    return tree


def choices(node):
    """Every way to satisfy the expression: a list of sets of identifiers (`OR` picks one side,
    `AND` needs both)."""
    kind, value = node
    if kind == "id":
        return [frozenset([value])]
    parts = [choices(child) for child in value]
    if kind == "or":
        return [choice for part in parts for choice in part]
    return [frozenset().union(*combo) for combo in itertools.product(*parts)]


def usable_choices(expression, policy):
    """The choices whose every license is allowed, the most preferred first. Empty: the package
    has no license that a product may ship."""
    found = [c for c in choices(parse(expression)) if c <= policy.allowed]
    return sorted(found, key=lambda c: (sorted((policy.rank(i) for i in c), reverse=True), sorted(c)))


# ---- license files ---------------------------------------------------------------------------


def is_license_file(name):
    return name.upper().startswith(LICENSE_PREFIXES) and not name.lower().endswith(SOURCE_SUFFIXES)


def is_notice_file(name):
    return name.upper().startswith(NOTICE_PREFIXES)


def classify(text):
    """The SPDX identifier a license file's text carries, or None when it is not one of the
    known texts (a combined or custom notice)."""
    flat = " ".join(text.split()).lower()
    for identifier, needles in SIGNATURES:
        if all(needle.lower() in flat for needle in needles):
            return identifier
    return None


def base_identifier(identifier):
    return identifier.split(" WITH ")[0]


def normal(text):
    """A text as the notices file keeps it: LF endings, no trailing blanks."""
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    return "\n".join(lines).strip("\n") + "\n"


def elect(expression, files, policy):
    """(identifiers, names of the files to ship) for a package. The alternative of the expression
    that is allowed and whose texts the package ships wins; if none has all of them, the one with
    most. A file that is a notice or that no signature recognises is always shipped. `files` is
    {name: text}. Raises `LicenseError` when no alternative is allowed."""
    options = usable_choices(expression, policy)
    if not options:
        raise LicenseError(f"{expression!r} has no alternative on the allow-list")
    kinds = {name: classify(text) for name, text in files.items() if not is_notice_file(name)}
    shipped = set(kinds.values())

    def covered(option):
        return sum(base_identifier(i) in shipped for i in option) - len(option)

    chosen = max(options, key=covered)  # max keeps the first (the most preferred) on a tie
    wanted = {base_identifier(i) for i in chosen}
    kept = [
        name for name, text in files.items()
        if is_notice_file(name) or kinds[name] in wanted | {None} or any(mark in name.upper() for mark in THIRD_PARTY_MARKS)
    ]
    return chosen, sorted(kept)


def missing_texts(chosen, files, kept):
    """The identifiers of the elected license that no kept file carries: their standard text is
    added."""
    present = {classify(files[name]) for name in kept if not is_notice_file(name)}
    return sorted(i for i in chosen if base_identifier(i) not in present)


def spdx_text(identifier, texts=SPDX_TEXTS):
    """The standard text of a license (without a holder's name)."""
    path = Path(texts) / f"{base_identifier(identifier)}.txt"
    if not path.is_file():
        raise LicenseError(f"scripts/third_party/spdx/{path.name} is missing: add the standard text")
    return normal(path.read_text())


# Where the terms of a short license begin; what comes before is the copyright statement.
TERMS_START = {
    "MIT": "Permission is hereby granted",
    "ISC": "Permission to use, copy",
    "BSD-2-Clause": "Redistribution and use in source and binary forms",
    "BSD-3-Clause": "Redistribution and use in source and binary forms",
    "Zlib": "This software is provided",
}
# Licenses whose text names the holder of the copyright in a placeholder.
NAMES_A_HOLDER = frozenset(TERMS_START) | {"0BSD", "MIT-0"}
TITLES = {
    "mit license", "the mit license", "the mit license (mit)", "mit", "isc license", "the isc license",
    "bsd 2-clause license", "bsd 3-clause license", "the bsd 3-clause license", "zlib license",
    "all rights reserved", "all rights reserved.",
}
APACHE_END = "END OF TERMS AND CONDITIONS"
APACHE_APPENDIX = re.compile(r"\s*APPENDIX: How to apply the Apache License.*?limitations under the License\.", re.S)
COPYRIGHT_LINE = re.compile(r"(?i)(copyright|\(c\)|\u00a9)")


def explode(identifier, body):
    """(copyright lines, [(label, text)]) of a license file. The terms of the common licenses are
    the same in every package that uses them, so they are kept once and the copyright lines go to
    the component; a file that does not split cleanly stays whole."""
    marker = TERMS_START.get(identifier)
    if marker and (at := body.find(marker)) > 0:
        lines = [line.strip() for line in body[:at].splitlines() if line.strip()]
        rights = [line for line in lines if COPYRIGHT_LINE.match(line)]
        if rights and all(line in rights or line.lower() in TITLES for line in lines):
            return rights, [(identifier, body[at:])]
    if identifier == "Apache-2.0" and (end := body.find(APACHE_END)) > 0:
        end += len(APACHE_END)
        tail = APACHE_APPENDIX.sub("", body[end:]).strip()
        parts = [(identifier, body[:end] + "\n")]
        lines = [line.strip() for line in tail.splitlines() if line.strip()]
        if lines and all(COPYRIGHT_LINE.match(line) for line in lines):
            return lines, parts
        return [], parts + ([("Apache-2.0 notice", tail + "\n")] if tail else [])
    return [], [(identifier, body)]


def allowing(policy, expression):
    """The policy with every license of `expression` allowed: for a package that has an entry in
    `exceptions` and is reported as one."""
    ids = frozenset().union(*choices(parse(expression)))
    return replace(policy, allowed=policy.allowed | ids)
