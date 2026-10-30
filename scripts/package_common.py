"""What the packaging scripts of every platform share: the error a step raises, the digest of a
file and the command runners (`package_mac.py`, `package_linux.py`)."""

from __future__ import annotations

import hashlib
import subprocess
from pathlib import Path

COMMAND_SECONDS = 600


class PackagingError(Exception):
    """A step failed; the message says which and why."""


def sha256_of(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_checksum(path):
    """`<file>.sha256` beside `path`, in the format `shasum -a 256 -c` reads."""
    path = Path(path)
    checksum = path.with_name(path.name + ".sha256")
    checksum.write_text(f"{sha256_of(path)}  {path.name}\n")
    return checksum


def _words(command):
    return [str(part) for part in command]


def run(command, *, check=True, timeout=COMMAND_SECONDS, environment=None, cwd=None):
    """Run a command, print it first, and return the finished process (output captured)."""
    print("+ " + " ".join(_words(command)), flush=True)
    result = subprocess.run(
        _words(command), capture_output=True, text=True, timeout=timeout, env=environment, cwd=cwd,
    )
    if check and result.returncode:
        detail = (result.stderr or result.stdout).strip()[-2000:]
        raise PackagingError(f"{command[0]} failed with status {result.returncode}:\n{detail}")
    return result


def stream(command, *, environment=None, cwd=None):
    """Run a long build with its output on the terminal."""
    print("+ " + " ".join(_words(command)), flush=True)
    status = subprocess.run(_words(command), env=environment, cwd=cwd).returncode
    if status:
        raise PackagingError(f"{command[0]} failed with status {status}")
