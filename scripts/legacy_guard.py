"""Refuse any identifier or path of the owner's legacy app (AGENTS.md, "Legacy app").

The legacy app is not this repository. Its bundle identifiers, its programs and its data
folders are listed once, in `contracts/legacy.json` (the generated form is `generated_legacy.py`;
the service seam's Rust guard reads its own generated form). Every identifier of the rewrite
carries "next". A script that writes an app, a service definition or a data directory passes what
it is about to touch through `require_next` or `require_next_path`; `check_legacy_isolation.py`
runs the same rules over the profile and the tracked files.
"""

from __future__ import annotations

import re
from pathlib import PurePath

from generated_legacy import APPLICATION_FOLDER, APPLICATIONS, BUNDLE_IDS, DATA_BASE, DATA_FOLDERS

MARKER = "next"
LEGACY_BUNDLE_IDS = BUNDLE_IDS
LEGACY_APPLICATIONS = tuple(f"{APPLICATION_FOLDER}/{name}" for name in APPLICATIONS)
LEGACY_DATA_FOLDERS = DATA_FOLDERS
APPLICATION_SUPPORT = DATA_BASE


def _any_of(values):
    # The longest first, so `Soyeht Dev` is not read as `Soyeht` followed by a word.
    return "|".join(re.escape(value) for value in sorted(values, key=len, reverse=True))


# A bundle identifier, a program or a data folder of the legacy app, in any text. A longer name
# that merely starts the same (`com.soyeht.mac.editor`) is the legacy app's too.
LEGACY_TEXT = re.compile(
    rf"(?:{_any_of(LEGACY_BUNDLE_IDS)})(?![A-Za-z0-9-])"
    rf"|(?:{_any_of(LEGACY_APPLICATIONS)})"
    rf"|{re.escape(APPLICATION_SUPPORT)}/(?:{_any_of(LEGACY_DATA_FOLDERS)})(?![.\w-])",
    re.IGNORECASE,  # The default Mac file system ignores case.
)


class LegacyIdentifier(SystemExit):
    """Raised, and exits the script, when a value could name the legacy app."""


def names_legacy(text: str) -> bool:
    return LEGACY_TEXT.search(text) is not None


def require_next(what: str, value: str) -> str:
    """Return `value` when it is an identifier of the rewrite, else stop the script."""
    if names_legacy(value) or MARKER not in value.lower():
        raise LegacyIdentifier(
            f'Refusing {what} "{value}": every identifier of the rewrite contains "{MARKER}", '
            "and the legacy app's must never be touched"
        )
    return value


def require_next_path(what: str, path: PurePath) -> PurePath:
    """Return `path` when it is not a location of the legacy app and names the rewrite."""
    text = path.as_posix()
    parts = [part.casefold() for part in path.parts]
    legacy_folders = {name.casefold() for name in LEGACY_DATA_FOLDERS}
    legacy_folder = any(
        parts[index] == APPLICATION_SUPPORT.casefold() and parts[index + 1] in legacy_folders
        for index in range(len(parts) - 1)
    )
    applications = [application.casefold() for application in LEGACY_APPLICATIONS]
    if (
        legacy_folder
        or any(text.casefold() == app or text.casefold().startswith(app + "/") for app in applications)
        or MARKER not in text.lower()
    ):
        raise LegacyIdentifier(
            f'Refusing {what} "{text}": a path of the rewrite contains "{MARKER}", '
            "and the legacy app's must never be touched"
        )
    return path
