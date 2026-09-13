#!/usr/bin/env python3
"""Install the new Dev profile without replacing any running service."""
import argparse
import os
import plistlib
import shutil
import socket
import subprocess
import time
from pathlib import Path

from generated_profile import (
    DEV_DATA_DIRECTORY_FROM_HOME,
    DEV_LAUNCHD_LABELS,
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


def service_environment():
    home = Path.home()
    command_paths = (*SYSTEM_PATHS, str(home / ".local/bin"))
    return {
        "HOME": str(home),
        "USER": os.environ.get("USER", ""),
        "PATH": os.pathsep.join(command_paths),
    }


def install_components(components, domain="gui"):
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
        if temporary.is_symlink():
            raise SystemExit("Dev temporary binary must not be a symlink")
        shutil.copy2(ROOT / f"engine/target/debug/soyeht-{component}", temporary)
        temporary.chmod(0o700)
        temporary.replace(installed)
    agents = Path.home() / "Library/LaunchAgents"
    agents.mkdir(parents=True, exist_ok=True)
    for component in components:
        label = DEV_LAUNCHD_LABELS[component]
        installed = DATA / f"bin/soyeht-{component}"
        temporary = installed.with_suffix(".new")
        if temporary.is_symlink():
            raise SystemExit("Dev temporary binary must not be a symlink")
        shutil.copy2(ROOT / f"engine/target/debug/soyeht-{component}", temporary)
        temporary.chmod(0o700)
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
        if temporary.is_symlink():
            raise SystemExit("Dev temporary CLI must not be a symlink")
        shutil.copy2(cli, temporary)
        temporary.chmod(0o700)
        temporary.replace(DATA / "bin/soyeht")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", choices=COMPONENTS, action="append")
    parser.add_argument("--domain", choices=("gui", "user"), default="gui")
    args = parser.parse_args()
    install_components(args.component or COMPONENTS, args.domain)


if __name__ == "__main__":
    main()
