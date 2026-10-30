#!/usr/bin/env python3
"""Install the new Dev profile without replacing any running service.

The signed programs are copied here; registering them with launchd is `soyeht service install`
(ADR 0114), the one place that writes a service definition.
"""
import argparse
import json
import os
import re
import shutil
import socket
import subprocess
import time
from pathlib import Path

from generated_profile import ENGINE_SOCKET, PROFILES, PTYD_SOCKET
from legacy_guard import require_next, require_next_path
from profile_programs import binary_identifiers

ROOT = Path(__file__).resolve().parents[1]
PROFILE = PROFILES["dev"]
DEV_LAUNCHD_LABELS = PROFILE["launchdLabels"]
DEV_SOCKET_NAMES = {"engine": ENGINE_SOCKET, "ptyd": PTYD_SOCKET}
DATA = Path.home() / "Library/Application Support" / PROFILE["dataDirectory"]["macos"]
COMPONENTS = ("ptyd", "engine")
ENGINE_SUPPORT = ("mcp",)
SERVICE_COMMAND_SECONDS = 60
BINARY_IDENTIFIERS = binary_identifiers(PROFILE)
IDENTITY_PATTERN = re.compile(
    r'^\s*\d+\)\s+([0-9A-F]{40})\s+"(Apple Development:[^"]+\([A-Z0-9]{10}\))"$'
)


def cli_path():
    return ROOT / "engine/target/debug/soyeht"


def service_command(*arguments):
    """Run `soyeht service ...` for the Dev profile and return the services it reports."""
    try:
        result = subprocess.run(
            [str(cli_path()), "--profile", "dev", "--data-dir", str(DATA), "service", *arguments],
            check=True, capture_output=True, text=True, timeout=SERVICE_COMMAND_SECONDS,
        )
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or "").strip() or f"exit status {error.returncode}"
        raise SystemExit(f"soyeht service {arguments[0]} failed: {detail}") from error
    return json.loads(result.stdout)["services"]


def signing_identities(output):
    return [match.groups() for line in output.splitlines()
            if (match := IDENTITY_PATTERN.match(line))]


def development_signing_identity(requested=None):
    result = subprocess.run(
        ["security", "find-identity", "-v", "-p", "codesigning"],
        check=True, capture_output=True, text=True, timeout=5,
    )
    identities = signing_identities(result.stdout)
    requested = requested or os.environ.get("SOYEHT_DEVELOPMENT_SIGNING_IDENTITY")
    if requested:
        matches = [item for item in identities if requested in item[:2]]
        if len(matches) != 1:
            raise SystemExit("The requested Apple Development signing identity is unavailable or ambiguous")
        return matches[0]
    if len(identities) != 1:
        raise SystemExit(
            "Set SOYEHT_DEVELOPMENT_SIGNING_IDENTITY to one available Apple Development identity"
        )
    return identities[0]


def copy_signed_binary(source, temporary, identifier, identity):
    if temporary.is_symlink():
        raise SystemExit("Dev temporary binary must not be a symlink")
    shutil.copy2(source, temporary)
    temporary.chmod(0o700)
    digest, name = identity
    subprocess.run(
        ["codesign", "--force", "--sign", digest, "--identifier", identifier,
         "--timestamp=none", str(temporary)],
        check=True, capture_output=True, timeout=30,
    )
    subprocess.run(
        ["codesign", "--verify", "--strict", "--verbose=2", str(temporary)],
        check=True, capture_output=True, timeout=10,
    )
    details = subprocess.run(
        ["codesign", "-dvvv", str(temporary)], capture_output=True, text=True,
        timeout=10,
    )
    requirement = subprocess.run(
        ["codesign", "-d", "-r-", str(temporary)], capture_output=True, text=True,
        timeout=10,
    )
    expected = (f"Identifier={identifier}", f"Authority={name}")
    team = re.search(r"^TeamIdentifier=([A-Z0-9]{10})$", details.stderr, re.MULTILINE)
    stable = (
        f'identifier "{identifier}"' in requirement.stdout
        and "anchor apple generic" in requirement.stdout
    )
    if details.returncode or requirement.returncode or not all(part in details.stderr for part in expected) or not team or not stable:
        temporary.unlink(missing_ok=True)
        raise SystemExit("Dev binary did not retain its stable signing identity")


def refuse_legacy(components):
    """Stop before anything is written when a value could name the legacy app."""
    require_next_path("data directory", DATA)
    for component in components:
        require_next("service label", DEV_LAUNCHD_LABELS[component])
    for component, identifier in BINARY_IDENTIFIERS.items():
        require_next(f"signing identifier of {component}", identifier)


def install_components(components, signing_identity=None):
    if not set(components).issubset(COMPONENTS):
        raise ValueError("Unsupported Dev service")
    refuse_legacy(components)
    if DATA.resolve() != DATA.absolute():
        raise SystemExit("Dev data path must not redirect through a symlink")
    if not cli_path().is_file():
        raise SystemExit("Build soyeht first")
    for component in components:
        (service,) = service_command("status", "--component", component)
        if service["running"]:
            raise SystemExit(f"Stop the new Dev {component} job before replacing its binary")
        if not (ROOT / f"engine/target/debug/soyeht-{component}").is_file():
            raise SystemExit(f"Build soyeht-{component} first")
    support = ENGINE_SUPPORT if "engine" in components else ()
    for component in support:
        if not (ROOT / f"engine/target/debug/soyeht-{component}").is_file():
            raise SystemExit(f"Build soyeht-{component} first")
    identity = development_signing_identity(signing_identity)
    DATA.mkdir(parents=True, exist_ok=True, mode=0o700)
    DATA.chmod(0o700)
    for folder in (DATA / "bin", DATA / "logs"):
        if folder.is_symlink():
            raise SystemExit("Dev folders must not be symlinks")
        folder.mkdir(exist_ok=True, mode=0o700)
        folder.chmod(0o700)
    for component in support:
        installed = DATA / f"bin/soyeht-{component}"
        temporary = installed.with_suffix(".new")
        copy_signed_binary(
            ROOT / f"engine/target/debug/soyeht-{component}", temporary,
            BINARY_IDENTIFIERS[component], identity,
        )
        temporary.replace(installed)
    for component in components:
        installed = DATA / f"bin/soyeht-{component}"
        temporary = installed.with_suffix(".new")
        copy_signed_binary(
            ROOT / f"engine/target/debug/soyeht-{component}", temporary,
            BINARY_IDENTIFIERS[component], identity,
        )
        temporary.replace(installed)
        service_command("install", "--component", component, "--programs", str(DATA / "bin"))
        deadline = time.monotonic() + 15
        while True:
            try:
                with socket.socket(socket.AF_UNIX) as probe:
                    probe.settimeout(1)
                    probe.connect(str(DATA / DEV_SOCKET_NAMES[component]))
                break
            except OSError:
                if time.monotonic() >= deadline:
                    raise SystemExit(f"New Dev {component} did not open its socket; inspect its private log")
                time.sleep(0.1)
        print(f"New Dev {component} installed")
    cli = cli_path()
    if cli.is_file():
        temporary = DATA / "bin/soyeht.new"
        copy_signed_binary(cli, temporary, BINARY_IDENTIFIERS["cli"], identity)
        temporary.replace(DATA / "bin/soyeht")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", choices=COMPONENTS, action="append")
    parser.add_argument("--signing-identity")
    args = parser.parse_args()
    install_components(args.component or COMPONENTS, args.signing_identity)


if __name__ == "__main__":
    main()
