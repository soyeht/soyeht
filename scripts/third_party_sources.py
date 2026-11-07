"""Where the third-party components come from (ADR 0149): the three cargo workspaces, the Swift
packages pinned in the `Package.resolved` files, the Gradle libraries of the Android app and the
fonts and icons shipped as files. Each source gives a `Component`: its name, version, license
expression, and the license and notice texts that go in the notices file.

The cargo sources read `cargo metadata --locked`, so the crates must be fetched (a build has done
it). The Swift and Gradle sources read only the repository: their texts are in
`scripts/third_party/` and `third_party_notices.py refresh-swift` renews them."""

from __future__ import annotations

import json
import os
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

import third_party_licenses as licenses

ROOT = Path(__file__).resolve().parents[1]
DATA = licenses.DATA
CARGO_WORKSPACES = ("engine", "apps/desktop", "apps/android")
RESOLVED_FILES = (
    "apps/shared/Package.resolved",
    "apps/dictation/Package.resolved",
    "apps/mac/SoyehtMac.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved",
    "apps/ios/SoyehtPhone.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved",
)
GRADLE_FILE = "apps/android/app/build.gradle.kts"
SWIFT_DATA = DATA / "swift.json"
SWIFT_TEXTS = DATA / "swift"
GRADLE_DATA = DATA / "gradle.json"
ASSETS_DATA = DATA / "assets.json"
METADATA_SECONDS = 600
MIN_LICENSE_BYTES = 200


class SourceError(Exception):
    """A dependency the data does not describe, or data that no longer matches the lock files."""


@dataclass(frozen=True)
class Text:
    label: str  # an SPDX identifier, or "NOTICE"
    body: str


@dataclass
class Component:
    ecosystem: str
    name: str
    version: str
    license: str
    url: str
    products: set = field(default_factory=set)  # the cargo workspaces that build it
    texts: list = field(default_factory=list)
    copyright: list = field(default_factory=list)  # the holders' statements, from the license files
    notes: list = field(default_factory=list)  # lines of the entry: a credit, the source of bundled code
    elected: list = field(default_factory=list)  # the license(s) of the expression that Soyeht follows
    exception: str = ""  # the reason, for a license outside the allow-list


def add_text(component, label, body, standard=False):
    """Put a license file's text in a component: a notice whole, the others with their copyright
    lines apart from the terms (see `licenses.explode`). The `standard` text of the SPDX list has
    a placeholder where a holder goes, so its copyright line is dropped."""
    if label == "NOTICE":
        component.texts.append(Text(label, body))
        return
    rights, parts = licenses.explode(label, body)
    if not standard:  # a REUSE-style text keeps `<year> <owner>` where a holder goes: that is no holder
        component.copyright += [line for line in rights if line not in component.copyright and not PLACEHOLDER.search(line)]
    component.texts += [Text(*part) for part in parts]


# ---- cargo -----------------------------------------------------------------------------------


def cargo_metadata(workspace, root=ROOT):
    cargo_bin = Path.home() / ".cargo/bin"
    environment = {**os.environ, "PATH": f"{cargo_bin}{os.pathsep}{os.environ.get('PATH', '')}"}
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"],
        cwd=Path(root) / workspace, capture_output=True, text=True, timeout=METADATA_SECONDS, env=environment,
    )
    if result.returncode:
        raise SourceError(
            f"cargo metadata failed in {workspace} (it runs offline, and needs every crate of the lock file, those of "
            f"the other systems too): run `cargo fetch --locked` in engine, apps/desktop and apps/android, or add "
            f"--fetch to this command.\n{result.stderr.strip()[-1200:]}"
        )
    return json.loads(result.stdout)


def fetch_crates(root=ROOT, workspaces=CARGO_WORKSPACES):
    """`cargo fetch --locked` in each workspace: every crate of the lock file, for every target."""
    cargo_bin = Path.home() / ".cargo/bin"
    environment = {**os.environ, "PATH": f"{cargo_bin}{os.pathsep}{os.environ.get('PATH', '')}"}
    for workspace in workspaces:
        done = subprocess.run(["cargo", "fetch", "--locked"], cwd=Path(root) / workspace, capture_output=True, text=True,
                              timeout=METADATA_SECONDS, env=environment)
        if done.returncode:
            raise SourceError(f"cargo fetch failed in {workspace}:\n{done.stderr.strip()[-1200:]}")


def shipped_packages(metadata):
    """The third-party packages a workspace's programs are built from: the closure of the normal
    dependencies of its members (not the dev-dependencies or the build tools), leaving out the
    members themselves and the path crates of this repository (those have no `source`)."""
    members = set(metadata["workspace_members"])
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    seen, pending = set(), sorted(members)
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        for dependency in nodes[current]["deps"]:
            if any(kind["kind"] is None for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    return sorted(
        (packages[i] for i in seen if i not in members and packages[i]["source"]),
        key=lambda p: (p["name"], p["version"]),
    )


def read_license_files(folder):
    """{name: text} of the license and notice files at the top of a package folder. A file of a
    few words is a link that the package archive lost (`../LICENSE-APACHE`), not a license."""
    found = {}
    for entry in sorted(Path(folder).iterdir()):
        if entry.is_file() and licenses.is_license_file(entry.name):
            body = licenses.normal(entry.read_text(errors="replace"))
            if len(body) >= MIN_LICENSE_BYTES or licenses.is_notice_file(entry.name):
                found[entry.name] = body
    return found


PLACEHOLDER = re.compile(r"<(year|owner|copyright|name)[^>]*>", re.I)
NESTED_DEPTH = 5
NESTED_SKIPPED = {"tests", "test", "benches", "examples", "target", "fuzz", ".git", "node_modules"}


def nested_license_files(folder):
    """{relative path: text} of the license files below the top of a package folder: the licenses of
    the code it bundles (a Unicode table, a C library, protocol descriptions), which travel with it
    whatever the package's own license is."""
    base, found = Path(folder), {}
    for path in sorted(base.rglob("*")):
        relative = path.relative_to(base)
        parts = relative.parts
        if len(parts) < 2 or len(parts) > NESTED_DEPTH or NESTED_SKIPPED & set(parts[:-1]) or not path.is_file():
            continue
        if licenses.is_license_file(parts[-1]):
            body = licenses.normal(path.read_text(errors="replace"))
            if len(body) >= MIN_LICENSE_BYTES:
                found[relative.as_posix()] = body
    return found


def holder_of(package):
    authors = [re.sub(r"\s*<[^>]*>", "", a).strip() for a in package.get("authors") or []]
    named = ", ".join(a for a in authors if a)
    return named or f"the {package['name']} developers ({package.get('repository') or 'see the package'})"


def _refuse_unread_copyleft(name, version, file, body):
    if not licenses.is_notice_file(Path(file).name) and licenses.names_copyleft(body):
        raise SourceError(
            f"{name} {version}: {file} names a GNU license in a form this check does not recognise: "
            "read it, and teach `SIGNATURES` its form or list the crate in policy.json"
        )


def cargo_component(package, policy):
    """The component of one cargo package, with the texts of its elected license."""
    name, declared = package["name"], package["license"]
    folder = Path(package["manifest_path"]).parent
    files = read_license_files(folder)
    if name in policy.clarify:
        declared = policy.clarify[name]["license"]
    if not declared:
        raise SourceError(f"{name} {package['version']} declares no license: add it to `clarify` in policy.json")
    exception = policy.exceptions.get((name, declared), "")
    if exception:
        policy = licenses.allowing(policy, declared)
    expression = declared
    bundled = policy.bundled.get(name)
    notice_only = bundled.get("notice_only", []) if bundled else []
    if bundled:
        # A package that compiles code of another license (freetype-sys builds FreeType): both apply.
        expression = f"({declared}) AND ({bundled['license']})"
        files.update({relative: licenses.normal((folder / relative).read_text()) for relative in bundled["files"]})
        if notice_only:  # licenses of bundled data, shipped as notices and not on the allow-list
            policy = licenses.allowing(policy, " AND ".join(notice_only))
    try:
        chosen, kept = licenses.elect(expression, files, policy)
    except licenses.LicenseError as error:
        raise SourceError(f"{name} {package['version']}: {error}") from error
    url = package.get("repository") or package.get("homepage") or ""
    component = Component("Rust", name, package["version"], expression, url, exception=exception, elected=sorted(chosen))
    if bundled:
        component.notes += [f"credit: {bundled['credit']}"] if bundled.get("credit") else []
        component.notes += [f"bundled source: {bundled['source']}"] if bundled.get("source") else []
    for file in kept:
        _refuse_unread_copyleft(name, package["version"], file, files[file])
        add_text(component, "NOTICE" if licenses.is_notice_file(file) else licenses.classify(files[file]) or declared, files[file])
    for identifier in licenses.missing_texts(chosen, files, kept):
        # The package ships no such file: the standard text, and the holder its metadata names.
        add_text(component, identifier, licenses.spdx_text(identifier), standard=True)
        if licenses.base_identifier(identifier) in licenses.NAMES_A_HOLDER:
            component.copyright.append(f"Copyright (c) {holder_of(package)} (named by the package metadata)")
    for relative, body in nested_license_files(folder).items():
        if relative in files:
            continue
        kind = licenses.classify(body)
        _refuse_unread_copyleft(name, package["version"], relative, body)
        if kind and not licenses.usable_choices(kind, policy) and kind not in notice_only:
            raise SourceError(
                f"{name} {package['version']} bundles {relative}, a {kind} text: {kind} is not on the allow-list; "
                "allow it for this package with `notice_only` in its `bundled` entry of policy.json, or drop the crate"
            )
        add_text(component, kind or "license of bundled code", body)
    return component


def cargo_components(policy, root=ROOT, workspaces=CARGO_WORKSPACES, metadata=cargo_metadata):
    found = {}
    for workspace in workspaces:
        for package in shipped_packages(metadata(workspace, root)):
            key = (package["name"], package["version"])
            if key not in found:
                found[key] = cargo_component(package, policy)
            found[key].products.add(workspace)
    return list(found.values())


# ---- Swift -----------------------------------------------------------------------------------


def resolved_pins(root=ROOT, files=RESOLVED_FILES):
    """{identity: (version or branch, revision, location)} of every pin of the lock files; two
    files pinning one package differently is an error."""
    pins = {}
    for relative in files:
        for pin in json.loads((Path(root) / relative).read_text())["pins"]:
            state = pin["state"]
            value = (state.get("version") or state.get("branch") or "", state["revision"], pin["location"])
            if pins.setdefault(pin["identity"], value) != value:
                raise SourceError(f"{pin['identity']} is pinned differently in {relative}")
    return pins


def swift_components(policy, root=ROOT):
    """The Swift packages, each checked against its pin: a package whose revision moved, or that
    has no recorded license, is a stale `swift.json` (renew it with `refresh-swift`)."""
    recorded = json.loads(SWIFT_DATA.read_text())["packages"]
    pins = resolved_pins(root)
    problems = [f"{i}: not in scripts/third_party/swift.json" for i in sorted(set(pins) - set(recorded))]
    problems += [f"{i}: in swift.json but pinned by no Package.resolved" for i in sorted(set(recorded) - set(pins))]
    components = []
    for identity in sorted(set(pins) & set(recorded)):
        version, revision, location = pins[identity]
        entry = recorded[identity]
        if entry["revision"] != revision:
            problems.append(f"{identity}: pinned at {revision[:10]}, swift.json has {entry['revision'][:10]}")
            continue
        options = licenses.usable_choices(entry["license"], policy)
        if not options:
            problems.append(f"{identity}: {entry['license']} is not on the allow-list")
            continue
        component = Component("Swift", identity, version or revision[:10], entry["license"], location,
                              elected=sorted(options[0]))
        for relative in entry["files"]:
            body = licenses.normal((SWIFT_TEXTS / identity / relative).read_text())
            kind = "NOTICE" if licenses.is_notice_file(Path(relative).name) else licenses.classify(body)
            if kind is None and licenses.names_copyleft(body):
                problems.append(f"{identity}: {relative} names a GNU license in a form this check does not recognise")
            add_text(component, kind or entry["license"], body)
        components.append(component)
    if problems:
        raise SourceError("swift.json does not match the Package.resolved files:\n  " + "\n  ".join(problems))
    return components


# ---- Gradle ----------------------------------------------------------------------------------


def gradle_declared(root=ROOT):
    """{coordinate: version} of the libraries the Android app ships (not the test ones), as
    `build.gradle.kts` writes them. A coordinate without a version takes it from the Compose BOM."""
    text = (Path(root) / GRADLE_FILE).read_text()
    variables = dict(re.findall(r'val (\w+) = "([^"]+)"', text))
    declared = {}
    for line in text.splitlines():
        if not re.match(r"\s*(implementation|api)\(", line) and "platform(" not in line:
            continue
        for coordinate in re.findall(r'"([\w.\-]+:[\w.\-]+(?::[^"]+)?)"', line):
            parts = coordinate.replace("@aar", "").split(":")
            version = parts[2] if len(parts) == 3 else ""
            declared[":".join(parts[:2])] = re.sub(r"\$(\w+)", lambda m: variables[m.group(1)], version)
    return declared


def gradle_components(policy, root=ROOT):
    data = json.loads(GRADLE_DATA.read_text())
    recorded = {l["coordinate"]: l for l in data["libraries"] if not l.get("transitive")}
    declared = gradle_declared(root)
    problems = [f"{c}: not in scripts/third_party/gradle.json" for c in sorted(set(declared) - set(recorded))]
    problems += [f"{c}: in gradle.json but not declared by build.gradle.kts" for c in sorted(set(recorded) - set(declared))]
    problems += [
        f"{c}: build.gradle.kts has {declared[c]!r}, gradle.json {recorded[c]['version']!r}"
        for c in sorted(set(declared) & set(recorded)) if declared[c] != recorded[c]["version"]
    ]
    if problems:
        raise SourceError("gradle.json does not match build.gradle.kts:\n  " + "\n  ".join(problems))
    components = []
    for library in sorted(data["libraries"], key=lambda l: l["coordinate"]):
        coordinate = library["coordinate"]
        chosen, _ = licenses.elect(library["license"], {}, policy)
        version = library["version"] or ("(Compose BOM)" if coordinate.startswith("androidx.compose.") else "")
        component = Component("Android", coordinate, version, library["license"], library["url"], elected=sorted(chosen))
        for identifier in sorted(chosen):
            add_text(component, identifier, licenses.spdx_text(identifier), standard=True)
        components.append(component)
    return components


# ---- fonts and icons -------------------------------------------------------------------------


def asset_components(policy, root=ROOT):
    components = []
    for asset in json.loads(ASSETS_DATA.read_text())["assets"]:
        options = licenses.usable_choices(asset["license"], policy)
        if not options:
            raise SourceError(f"{asset['name']}: {asset['license']} is not on the allow-list")
        body = licenses.normal((Path(root) / asset["file"]).read_text())
        component = Component("Asset", asset["name"], asset["version"], asset["license"], asset["url"], elected=sorted(options[0]))
        add_text(component, asset["license"], body)
        components.append(component)
    return components
