"""The operating-system seams of the end-to-end scripts.

Mirrors `engine/soyeht-platform` for Python: how a client reaches a local endpoint (a Unix
socket, or on Windows the loopback endpoint file of contract 08, section 2.5), how services
are started and stopped (launchd on macOS, plain child processes everywhere else), and where
files go. Nothing else in the e2e scripts asks which system it runs on.
"""

import hashlib
import hmac
import json
import os
import shlex
import shutil
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
from contextlib import closing, contextmanager
from pathlib import Path

WINDOWS = sys.platform == "win32"
MACOS = sys.platform == "darwin"
# Force a launcher by name, for tests: "launchd" or "process".
LAUNCHER_ENV = "SOYEHT_E2E_LAUNCHER"
LAUNCHERS = ("launchd", "process")
ENDPOINT_MAGIC = "soyeht-local-2"
NONCE_BYTES = 32
STOP_SECONDS = 10


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def executable(name):
    """The file name of a program built by cargo."""
    return f"{name}.exe" if WINDOWS else name


def short_temp_root():
    """A parent for data directories, short enough for a Unix socket path (104 bytes on macOS)."""
    return Path(tempfile.gettempdir()) if WINDOWS else Path("/tmp")


def user_cache_directory():
    """The user's cache folder; created on demand."""
    if MACOS:
        path = Path.home() / "Library/Caches"
    elif WINDOWS:
        path = Path(os.environ.get("LOCALAPPDATA") or Path.home() / "AppData/Local")
    else:
        path = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    path.mkdir(parents=True, exist_ok=True)
    return path


def raise_file_limit():
    """Raise the open-file limit toward 4096 where the system has one."""
    try:
        import resource
    except ImportError:
        return
    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    ceiling = 4096 if hard == resource.RLIM_INFINITY else hard
    target = min(4096, ceiling)
    if soft < target:
        resource.setrlimit(resource.RLIMIT_NOFILE, (target, hard))


@contextmanager
def database(path, timeout=5):
    """A SQLite connection that commits and is closed on exit. `with sqlite3.connect(...)`
    only ends the transaction and leaves the file open, which Windows will not let go of."""
    with closing(sqlite3.connect(path, timeout=timeout)) as connection, connection:
        yield connection


def remove_directory(path, seconds=10):
    """Remove a run's data folder. Windows refuses while a just-exited process still holds a
    file open, so a sharing violation is retried for a short, bounded time."""
    deadline = time.monotonic() + seconds
    while True:
        try:
            shutil.rmtree(path)
            return
        except OSError:
            if not WINDOWS or time.monotonic() >= deadline:
                raise
            time.sleep(0.2)


def write_launcher_script(path, environment, argv):
    """An executable that sets `environment`, then runs `argv` with its own arguments appended.

    A shell script, or on Windows a `.cmd` file (so the suffix is replaced); returns the path
    written.
    """
    if WINDOWS:
        path = path.with_suffix(".cmd")
        lines = ["@echo off"]
        for key, value in environment.items():
            require('"' not in value, f"{key} cannot hold a quote in a .cmd file")
            lines.append(f'set "{key}={value.replace("%", "%%")}"')
        lines.append(subprocess.list2cmdline(argv) + " %*")
        path.write_bytes(("\r\n".join(lines) + "\r\n").encode())
        return path
    lines = [f"export {key}={shlex.quote(value)}" for key, value in environment.items()]
    lines.append("exec " + shlex.join(argv) + ' "$@"')
    path.write_text("#!/bin/sh\n" + "\n".join(lines) + "\n")
    path.chmod(0o700)
    return path


def read_exact(stream, size):
    result = bytearray()
    while len(result) < size:
        chunk = stream.recv(size - len(result))
        if not chunk:
            raise EOFError("service disconnected")
        result.extend(chunk)
    return bytes(result)


def endpoint_proof(secret, side, client_nonce, server_nonce):
    return hmac.new(secret, f"{ENDPOINT_MAGIC} {side}".encode() + client_nonce + server_nonce,
                    hashlib.sha256).digest()


def connect_loopback(path, timeout):
    """Connect through an endpoint file: `soyeht-local-2 <port> <secret>` (contract 08, 2.5)."""
    magic, port, secret = path.read_text().split()
    require(magic == ENDPOINT_MAGIC, "unknown endpoint file format")
    secret = bytes.fromhex(secret)
    stream = socket.create_connection(("127.0.0.1", int(port)), timeout=timeout)
    try:
        client_nonce = os.urandom(NONCE_BYTES)
        stream.sendall(client_nonce)
        reply = read_exact(stream, NONCE_BYTES + hashlib.sha256().digest_size)
        server_nonce, proof = reply[:NONCE_BYTES], reply[NONCE_BYTES:]
        if not hmac.compare_digest(
                proof, endpoint_proof(secret, "server", client_nonce, server_nonce)):
            raise PermissionError("the endpoint does not know the secret")
        stream.sendall(endpoint_proof(secret, "client", client_nonce, server_nonce))
    except BaseException:
        stream.close()
        raise
    return stream


def connect_endpoint(path, timeout=10):
    """A stream to the engine or supervisor endpoint at `path`, whatever the system's flavour."""
    path = Path(path)
    if not path.exists():
        raise FileNotFoundError(str(path))
    if path.is_file():
        return connect_loopback(path, timeout)
    stream = socket.socket(socket.AF_UNIX)
    stream.settimeout(timeout)
    try:
        stream.connect(str(path))
    except BaseException:
        stream.close()
        raise
    return stream


def launcher_kind(environ=None, platform=None, launchctl=None):
    """launchd where the system has it, plain child processes elsewhere; `LAUNCHER_ENV` forces one."""
    environ = os.environ if environ is None else environ
    platform = sys.platform if platform is None else platform
    launchctl = shutil.which("launchctl") if launchctl is None else launchctl
    forced = environ.get(LAUNCHER_ENV)
    if forced is not None:
        require(forced in LAUNCHERS, f"{LAUNCHER_ENV} must be one of {', '.join(LAUNCHERS)}")
        require(forced != "launchd" or launchctl, "launchd is not available on this system")
        return forced
    return "launchd" if platform == "darwin" and launchctl else "process"


class Launcher:
    """Starts, stops and crashes the services of one run. A component is "ptyd" or "engine"."""

    def define(self, component, argv, environment, log):
        raise NotImplementedError

    def exists(self, component):
        raise NotImplementedError

    def start(self, component):
        raise NotImplementedError

    def stop(self, component):
        """Ask the service to end; return when it is gone."""
        raise NotImplementedError

    def crash(self, component):
        """Kill the service without warning. A service manager brings it back, as launchd does."""
        raise NotImplementedError

    def take_down(self, component):
        """Kill the service and keep the service manager from bringing it back."""
        raise NotImplementedError

    def wait_gone(self, component, seconds=20):
        deadline = time.monotonic() + seconds
        while self.exists(component):
            require(time.monotonic() < deadline, f"the {component} did not end")
            time.sleep(0.05)


class LaunchdLauncher(Launcher):
    # launchd services start with a minimal environment, not the user's.
    base_path = "/usr/bin:/bin:/usr/sbin:/sbin"

    def __init__(self, data, prefix, cli):
        self.data, self.prefix, self.cli = data, prefix, cli
        self.domain = f"gui/{os.getuid()}"
        if subprocess.run(["launchctl", "print", self.domain], capture_output=True).returncode:
            self.domain = f"user/{os.getuid()}"
        subprocess.run(["launchctl", "print", self.domain], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def target(self, component):
        return f"{self.domain}/{self.prefix}.{component}"

    def define(self, component, argv, environment, log):
        # The text of the agent is `soyeht service render`'s: no script keeps a copy of it.
        name = f"{self.prefix}.{component}"
        variables = {"HOME": str(Path.home()), "PATH": self.base_path, **environment}
        command = [str(self.cli), "service", "render", "--system", "macos", "--name", name,
                   "--description", f"Soyeht {component} (disposable)", "--log", str(log)]
        for key, value in variables.items():
            command += ["--env", f"{key}={value}"]
        command += ["--", *(str(argument) for argument in argv)]
        text = json.loads(subprocess.run(
            command, check=True, capture_output=True, text=True).stdout)["text"]
        log.touch(mode=0o600)
        destination = self.data / f"{component}.plist"
        destination.write_text(text, encoding="utf-8")
        destination.chmod(0o600)

    def exists(self, component):
        return subprocess.run(["launchctl", "print", self.target(component)],
                              capture_output=True).returncode == 0

    def start(self, component):
        subprocess.run(["launchctl", "bootstrap", self.domain,
                        str(self.data / f"{component}.plist")], check=True)

    def stop(self, component):
        subprocess.run(["launchctl", "bootout", self.target(component)], check=True)
        # bootout can return before launchd forgets the label; a busy host shows the old
        # service to the next start's "already exists" check.
        self.wait_gone(component)

    def crash(self, component):
        subprocess.run(["launchctl", "kill", "SIGKILL", self.target(component)], check=True)

    def take_down(self, component):
        # Disable only this randomly named job so the test can inspect the outage before
        # launchd loads a replacement.
        target = self.target(component)
        subprocess.run(["launchctl", "disable", target], check=True)
        try:
            self.crash(component)
            subprocess.run(["launchctl", "bootout", target], check=True)
            self.wait_gone(component)
        finally:
            subprocess.run(["launchctl", "enable", target], check=True)


class ProcessLauncher(Launcher):
    """Plain child processes in their own session (POSIX) or process group (Windows)."""

    def __init__(self, data):
        self.data = data
        self.base_path = os.environ.get("PATH", "")
        self.definitions = {}
        self.processes = {}

    def define(self, component, argv, environment, log):
        log.touch(mode=0o600)
        self.definitions[component] = (
            [str(argument) for argument in argv], {**os.environ, **environment}, log)

    def exists(self, component):
        process = self.processes.get(component)
        return process is not None and process.poll() is None

    def start(self, component):
        argv, environment, log = self.definitions[component]
        flags = ({"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP
                  | subprocess.CREATE_NO_WINDOW} if WINDOWS else {"start_new_session": True})
        with open(log, "ab") as output:
            self.processes[component] = subprocess.Popen(
                argv, env=environment, cwd=self.data, stdin=subprocess.DEVNULL,
                stdout=output, stderr=subprocess.STDOUT, **flags)

    def end(self, component, kill):
        """End the service and its whole process tree, and return only when it has exited."""
        process = self.processes[component]
        if process.poll() is None:
            if WINDOWS:
                # Windows has no graceful signal; /T takes the children down with it.
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                               capture_output=True, check=False)
            else:
                process.kill() if kill else process.terminate()
        try:
            process.wait(STOP_SECONDS)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        if not WINDOWS:
            # The service ran in a session of its own; sweep what it left behind.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass

    def stop(self, component):
        self.end(component, kill=False)

    def crash(self, component):
        self.end(component, kill=True)
        self.start(component)

    def take_down(self, component):
        self.end(component, kill=True)


def make_launcher(data, prefix, cli):
    """The launcher for this system. `cli` is the built `soyeht` program, which renders agents."""
    if launcher_kind() == "launchd":
        return LaunchdLauncher(data, prefix, cli)
    return ProcessLauncher(data)
