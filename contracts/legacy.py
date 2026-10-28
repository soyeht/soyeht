"""Renders the list of the legacy app's names and places that every guard refuses.

The owner's older app is not this repository (ADR 0112). Its bundle identifiers, programs and
data folders are listed once, in `contracts/legacy.json`; the scripts' guard
(`scripts/legacy_guard.py`) and the service seam's guard (`soyeht_service::guard`) read the
generated forms and keep no list of their own.
"""

import json
import subprocess

from generated_header import header_text

LEGACY_SOURCE = "contracts/legacy.json"
RUST_OUTPUT = "engine/soyeht-protocol/src/legacy.rs"
PYTHON_OUTPUT = "scripts/generated_legacy.py"
FIELDS = ("version", "bundleIds", "applicationFolder", "applications", "dataBase", "dataFolders")


def validate_legacy(document):
    if set(document) != set(FIELDS) or document["version"] != 1:
        raise ValueError(f"legacy.json must have version 1 and exactly {', '.join(FIELDS[1:])}")
    for field in ("bundleIds", "applications", "dataFolders"):
        values = document[field]
        if not values or len(set(values)) != len(values):
            raise ValueError(f"legacy {field} must be a non-empty list without repeats")
    for value in [*document["bundleIds"], *document["applications"], *document["dataFolders"],
                  document["applicationFolder"], document["dataBase"]]:
        if not isinstance(value, str) or not value or not value.isprintable():
            raise ValueError("legacy values must be printable text")
    if not all(name.endswith(".app") and "/" not in name for name in document["applications"]):
        raise ValueError("legacy applications are folder names ending in .app")
    if any("/" in name for name in document["dataFolders"]):
        raise ValueError("legacy data folders are single folder names")


def _rust_list(name, values, doc):
    items = ", ".join(json.dumps(value) for value in values)
    return f"/// {doc}\npub const {name}: [&str; {len(values)}] = [{items}];\n"


def render_legacy(document):
    header = header_text(LEGACY_SOURCE)
    rust = f"// {header}\n"
    rust += _rust_list("BUNDLE_IDS", document["bundleIds"], "Bundle identifiers; a longer name that starts with one is the legacy app's too.")
    rust += f'/// The folder the legacy programs are installed in.\npub const APPLICATION_FOLDER: &str = {json.dumps(document["applicationFolder"])};\n'
    rust += _rust_list("APPLICATIONS", document["applications"], "Folder names of the legacy programs; anything inside one is the legacy app's.")
    rust += f'/// The folder that holds application data, below which the legacy app keeps its own.\npub const DATA_BASE: &str = {json.dumps(document["dataBase"])};\n'
    rust += _rust_list("DATA_FOLDERS", document["dataFolders"], "The legacy app's data folders, below `DATA_BASE`.")
    python = f"# {header}\n"
    for name, field in (("BUNDLE_IDS", "bundleIds"), ("APPLICATIONS", "applications"),
                        ("DATA_FOLDERS", "dataFolders")):
        python += f"{name} = {tuple(document[field])!r}\n"
    python += f"APPLICATION_FOLDER = {document['applicationFolder']!r}\n"
    python += f"DATA_BASE = {document['dataBase']!r}\n"
    formatted = subprocess.run(["rustfmt", "--edition", "2024"], input=rust, text=True,
                               capture_output=True, check=True).stdout
    return {RUST_OUTPUT: formatted, PYTHON_OUTPUT: python}
