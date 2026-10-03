#!/usr/bin/env python3
"""Keep the approved pane interior flat and palette-only.

The pane shell is the only neumorphic surface (`paneShell()` on the Mac). Inside a pane the
approved design ("Interior da pane · APROVADO", design/soyeht-desktop.pen) is flat: no raised
or inset shadows, every color is a palette role and the interface face is Inter. This gate
rejects neumorphic modifiers, shadows, literal colors and the window chrome font in the pane
interior sources.
"""

from __future__ import annotations

import pathlib
import re
import sys

from check_mac_view_colors import SYSTEM_COLOR, code_only

# Whole files that draw only pane interiors.
INTERIOR_FILES = (
    "apps/shared/Sources/SoyehtClient/AgentConversation.swift",
    "apps/shared/Sources/SoyehtClient/AgentConversationCards.swift",
    "apps/shared/Sources/SoyehtClient/AgentConversationStyle.swift",
    "apps/shared/Sources/SoyehtClient/AgentComposerAttachmentViews.swift",
    "apps/shared/Sources/SoyehtClient/AgentPermissionModeMenu.swift",
    "apps/shared/Sources/SoyehtClient/AgentSubagents.swift",
    "apps/shared/Sources/SoyehtClient/PermissionPresentationView.swift",
    "apps/shared/Sources/SoyehtClient/PaneInteriorControls.swift",
    "apps/shared/Sources/SoyehtClient/SlashCompletionList.swift",
    "apps/mac/Sources/AgentHarnessPicker.swift",
    "apps/mac/Sources/PaneHarnessControl.swift",
    "apps/mac/Sources/EditorFindBar.swift",
    "apps/mac/Sources/EditorHistoryViews.swift",
    "apps/mac/Sources/EditorMarkdownPreview.swift",
    "apps/mac/Sources/EditorPaneView.swift",
    "apps/mac/Sources/EditorProjectPanel.swift",
    "apps/mac/Sources/EditorStatusBar.swift",
    "apps/mac/Sources/EditorTabBar.swift",
    "apps/mac/Sources/PaneTabStrip.swift",
    "apps/mac/Sources/Web/WebPaneSurface.swift",
    "apps/mac/Sources/Web/WebWorkspace.swift",
)
# Files that mix window chrome with pane interiors: only these types are pane interiors.
INTERIOR_TYPES = {
    "apps/mac/Sources/WorkspaceWindow.swift": (
        "PaneCard",
        "NewPaneSheet",
        "AgentHarnessSwitchPicker",
    ),
}

NEUMORPHIC = re.compile(
    r"\.neoRaised\s*\(|\.neoWell\s*\(|\.neoAccent\s*\(|\bNeoPillButtonStyle\b"
    r"|\.shadow\s*\(|\.inner\s*\(\s*color"
)
RAW_COLOR = re.compile(
    rf"\bColor\s*\(|\b(?:NS|UI)Color\s*[.(]|\bColor\s*\.\s*{SYSTEM_COLOR}\b"
    rf"|\.(?:foregroundStyle|foregroundColor|background|fill|stroke|tint|border)"
    rf"\s*\(\s*\.{SYSTEM_COLOR}\b"
)
HEX_LITERAL = re.compile(r'"#[0-9A-Fa-f]{6}(?:[0-9A-Fa-f]{2})?"')
# The interior speaks Inter (`MacTheme.ui`); Nunito (`MacTheme.chrome`) stays in the window chrome.
CHROME_FONT = re.compile(r"\bMacTheme\.chrome\s*\(")
TYPE_DECLARATION = r"(?:struct|final\s+class|class|enum|extension)\s+{name}\b[^{{]*\{{"


def strip_comments(source: str) -> str:
    """Blank comments but keep string literals, so a hex string stays visible."""
    result = list(source)
    index = 0
    quote: str | None = None
    while index < len(source):
        pair = source[index : index + 2]
        if quote:
            if source.startswith(quote, index) and source[index - 1] != "\\":
                index += len(quote)
                quote = None
                continue
            index += 1
        elif source.startswith('"""', index) or source[index] == '"':
            quote = '"""' if source.startswith('"""', index) else '"'
            index += len(quote)
        elif pair in ("//", "/*"):
            end = source.find("\n" if pair == "//" else "*/", index + 2)
            end = len(source) if end < 0 else end + (0 if pair == "//" else 2)
            for position in range(index, end):
                if source[position] != "\n":
                    result[position] = " "
            index = end
        else:
            index += 1
    return "".join(result)


def type_spans(code: str, names: tuple[str, ...]) -> list[tuple[int, int]]:
    spans: list[tuple[int, int]] = []
    for name in names:
        match = re.search(TYPE_DECLARATION.format(name=re.escape(name)), code)
        if match is None:
            spans.append((-1, -1))
            continue
        depth = 0
        for index in range(match.end() - 1, len(code)):
            if code[index] == "{":
                depth += 1
            elif code[index] == "}":
                depth -= 1
                if depth == 0:
                    spans.append((match.start(), index + 1))
                    break
    return spans


def violations(source: str, spans: list[tuple[int, int]] | None = None) -> list[tuple[int, str]]:
    code = code_only(source)
    with_strings = strip_comments(source)
    found = [(m.start(), "neumorphic shadow inside a pane") for m in NEUMORPHIC.finditer(code)]
    found += [(m.start(), "color outside the palette roles") for m in RAW_COLOR.finditer(code)]
    found += [(m.start(), "window chrome font inside a pane") for m in CHROME_FONT.finditer(code)]
    found += [
        (m.start(), "literal hex color") for m in HEX_LITERAL.finditer(with_strings)
    ]
    if spans is not None:
        found = [item for item in found if any(start <= item[0] < end for start, end in spans)]
    return sorted({(source.count("\n", 0, offset) + 1, reason) for offset, reason in found})


def scan(root: pathlib.Path = pathlib.Path(".")) -> list[str]:
    problems: list[str] = []
    for relative in INTERIOR_FILES:
        path = root / relative
        if path.is_file():
            for line, reason in violations(path.read_text(encoding="utf-8")):
                problems.append(f"{relative}:{line}: {reason}")
    for relative, names in INTERIOR_TYPES.items():
        path = root / relative
        if not path.is_file():
            continue
        source = path.read_text(encoding="utf-8")
        spans = type_spans(code_only(source), names)
        for name, span in zip(names, spans):
            if span == (-1, -1):
                problems.append(f"{relative}: pane interior type {name} not found")
        for line, reason in violations(source, [span for span in spans if span[0] >= 0]):
            problems.append(f"{relative}:{line}: {reason}")
    return problems


def main() -> int:
    problems = scan()
    for problem in problems:
        print(f"pane interior: {problem}; use paneShell, paneCard, paneWell or palette roles")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
