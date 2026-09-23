#!/usr/bin/env python3
"""Check that the pane-drag type shipped in the Mac app matches its Swift source."""

import plistlib
import re
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check_pane_drag_type.py <built-app-Info.plist>", file=sys.stderr)
        return 2

    root = Path(__file__).resolve().parent.parent
    policy = root / "apps/shared/Sources/SoyehtClient/PaneLayoutInteractionGeometry.swift"
    source_plist = root / "apps/mac/Resources/Info.plist"
    built_plist = Path(sys.argv[1])
    match = re.search(
        r'public static let typeIdentifier = "([^"]+)"', policy.read_text()
    )
    if match is None:
        print("PaneDragTransferPolicy.typeIdentifier not found; restore the single Swift identifier.", file=sys.stderr)
        return 1

    expected = match.group(1)
    for path in (source_plist, built_plist):
        with path.open("rb") as stream:
            plist = plistlib.load(stream)
        declarations = plist.get("UTExportedTypeDeclarations", [])
        matches = [
            item for item in declarations if item.get("UTTypeIdentifier") == expected
        ]
        if len(matches) != 1 or matches[0].get("UTTypeConformsTo") != ["public.data"]:
            print(
                f"{path}: declare {expected} exactly once as an exported public.data type; "
                "keep it in sync with PaneDragTransferPolicy.typeIdentifier.",
                file=sys.stderr,
            )
            return 1
    print(f"pane drag type: {expected} declared in source and built Info.plist")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
