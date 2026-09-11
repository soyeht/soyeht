#!/usr/bin/env python3
"""Install only the new Dev supervisor; refuse to replace a running job."""
import os
import plistlib
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = Path.home() / "Library/Application Support/Soyeht.next/dev"
LABEL = "com.soyeht.next.dev.ptyd"


def main():
    service = f"gui/{os.getuid()}/{LABEL}"
    running = subprocess.run(["launchctl", "print", service], capture_output=True)
    if running.returncode == 0:
        raise SystemExit("Dev supervisor already installed; stop the Dev job explicitly before replacing its binary")
    binary = ROOT / "engine/target/debug/soyeht-ptyd"
    if not binary.is_file():
        raise SystemExit("Build soyeht-ptyd first")
    DATA.mkdir(parents=True, exist_ok=True, mode=0o700)
    DATA.chmod(0o700)
    for folder in (DATA / "bin", DATA / "logs"):
        folder.mkdir(exist_ok=True, mode=0o700)
    installed = DATA / "bin/soyeht-ptyd"
    shutil.copy2(binary, installed)
    installed.chmod(0o700)
    template = ROOT / f"engine/soyeht-ptyd/launchd/{LABEL}.plist"
    values = plistlib.loads(template.read_bytes())
    values["ProgramArguments"] = [str(installed), "--data-dir", str(DATA)]
    for key in ("StandardOutPath", "StandardErrorPath"):
        values[key] = str(DATA / "logs/ptyd.log")
    log = DATA / "logs/ptyd.log"
    log.touch(mode=0o600, exist_ok=True)
    log.chmod(0o600)
    destination = Path.home() / f"Library/LaunchAgents/{LABEL}.plist"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(plistlib.dumps(values))
    destination.chmod(0o600)
    subprocess.run(["launchctl", "bootstrap", f"gui/{os.getuid()}", str(destination)], check=True)
    subprocess.run(["launchctl", "print", service], check=True, stdout=subprocess.DEVNULL)
    print("Dev supervisor installed")


if __name__ == "__main__":
    main()
