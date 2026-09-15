#!/usr/bin/env python3
"""Install the new Dev profile without replacing any running service."""
import argparse
import os
import plistlib
import re
import shutil
import socket
import subprocess
import time
from pathlib import Path

from generated_profile import (
    DEV_DATA_DIRECTORY_FROM_HOME,
    DEV_LAUNCHD_LABELS,
    DEV_LAUNCHD_PREFIX,
    DEV_SOCKET_NAMES,
)

ROOT = Path(__file__).resolve().parents[1]
DATA = Path.home() / DEV_DATA_DIRECTORY_FROM_HOME
COMPONENTS = ("ptyd", "engine")
ENGINE_SUPPORT = ("mcp",)
SYSTEM_PATHS = (
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
)
BINARY_IDENTIFIERS = {
    **DEV_LAUNCHD_LABELS,
    "mcp": f"{DEV_LAUNCHD_PREFIX}.mcp",
    "cli": f"{DEV_LAUNCHD_PREFIX}.cli",
}
IDENTITY_PATTERN = re.compile(
    r'^\s*\d+\)\s+([0-9A-F]{40})\s+"(Apple Development:[^"]+\([A-Z0-9]{10}\))"$'
)


def service_environment():
    home = Path.home()
    command_paths = (
        *SYSTEM_PATHS,
        str(home / ".local/bin"),
        str(home / ".cargo/bin"),
    )
    return {
        "HOME": str(home),
        "USER": os.environ.get("USER", ""),
        "PATH": os.pathsep.join(command_paths),
    }


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


def install_components(components, domain="gui", signing_identity=None):
    if not set(components).issubset(COMPONENTS) or domain not in ("gui", "user"):
        raise ValueError("Unsupported Dev service")
    if DATA.resolve() != DATA.absolute():
        raise SystemExit("Dev data path must not redirect through a symlink")
    target = f"{domain}/{os.getuid()}"
    subprocess.run(["launchctl", "print", target], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for component in components:
        label = DEV_LAUNCHD_LABELS[component]
        running = subprocess.run(["launchctl", "print", f"{target}/{label}"], capture_output=True)
        if running.returncode == 0:
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
    agents = Path.home() / "Library/LaunchAgents"
    agents.mkdir(parents=True, exist_ok=True)
    for component in components:
        label = DEV_LAUNCHD_LABELS[component]
        installed = DATA / f"bin/soyeht-{component}"
        temporary = installed.with_suffix(".new")
        copy_signed_binary(
            ROOT / f"engine/target/debug/soyeht-{component}", temporary,
            BINARY_IDENTIFIERS[component], identity,
        )
        temporary.replace(installed)
        values = plistlib.loads(
            (ROOT / f"engine/soyeht-{component}/launchd/dev.plist").read_bytes()
        )
        values["ProgramArguments"] = [str(installed), "--data-dir", str(DATA)]
        values["EnvironmentVariables"] = service_environment()
        log = DATA / f"logs/{component}.log"
        if log.is_symlink():
            raise SystemExit("Dev log must not be a symlink")
        log.touch(mode=0o600, exist_ok=True)
        log.chmod(0o600)
        values["StandardOutPath"] = values["StandardErrorPath"] = str(log)
        destination = agents / f"{label}.plist"
        temporary_plist = destination.with_suffix(".new")
        if destination.is_symlink() or temporary_plist.is_symlink():
            raise SystemExit("Dev service definition must not be a symlink")
        temporary_plist.write_bytes(plistlib.dumps(values))
        temporary_plist.chmod(0o600)
        temporary_plist.replace(destination)
        subprocess.run(["launchctl", "bootstrap", target, str(destination)], check=True)
        subprocess.run(["launchctl", "print", f"{target}/{label}"], check=True,
                       stdout=subprocess.DEVNULL)
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
    cli = ROOT / "engine/target/debug/soyeht"
    if cli.is_file():
        temporary = DATA / "bin/soyeht.new"
        copy_signed_binary(cli, temporary, BINARY_IDENTIFIERS["cli"], identity)
        temporary.replace(DATA / "bin/soyeht")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", choices=COMPONENTS, action="append")
    parser.add_argument("--domain", choices=("gui", "user"), default="gui")
    parser.add_argument("--signing-identity")
    args = parser.parse_args()
    install_components(args.component or COMPONENTS, args.domain, args.signing_identity)


if __name__ == "__main__":
    main()
