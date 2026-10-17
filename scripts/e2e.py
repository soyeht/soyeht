#!/usr/bin/env python3
"""Run the six slice-one acceptances with isolated services (launchd on macOS)."""
import argparse
import base64
import hashlib
import json
import os
import struct
import subprocess
import tempfile
import time
import uuid
from pathlib import Path

from e2e_platform import (
    MACOS,
    connect_endpoint,
    executable,
    make_launcher,
    read_exact,
    remove_directory,
    require,
    short_temp_root,
)
from generated_profile import (
    DEV_ENGINE_SOCKET,
    DEV_LAUNCHD_PREFIX,
    DEV_PTYD_SOCKET,
)

ROOT = Path(__file__).resolve().parents[1]
PRODUCT_EXECUTABLES = tuple(executable(name) for name in
                            ("soyeht", "soyeht-engine", "soyeht-mcp", "soyeht-ptyd"))
# The engine with the test hooks installed (ADR 0091); built with `--features test-hooks`.
TEST_ENGINE = "soyeht-engine-test"


def send(stream, value):
    body = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode()
    require(len(body) <= 1048576, "test request exceeds JSON limit")
    stream.sendall(struct.pack(">BI", 1, len(body)) + body)


def receive(stream):
    kind, size = struct.unpack(">BI", read_exact(stream, 5))
    maximum = {1: 1048576, 2: 16 + 65536, 3: 16 + 262144}.get(kind)
    require(maximum is not None and size <= maximum, "invalid frame header")
    body = read_exact(stream, size)
    if kind == 1:
        return json.loads(body)
    require(kind == 2 and size >= 16, "unexpected binary frame")
    pane_ref, generation, offset = struct.unpack(">IIQ", body[:16])
    return {"binaryPaneRef": pane_ref, "generation": generation,
            "offset": offset, "bytes": body[16:]}


class Client:
    def __init__(self, data, name="acceptance", capabilities=None):
        self.stream = connect_endpoint(data / DEV_ENGINE_SOCKET)
        try:
            require(receive(self.stream).get("method") == "auth.challenge", "missing challenge")
            self.next = 0
            self.pending = []
            hello = {"client": name, "clientVersion": "1", "protocolVersion": 1}
            if capabilities is not None:
                hello["capabilities"] = capabilities
            self.welcome = self.call("hello", hello)
        except BaseException:
            self.stream.close()
            raise

    def close(self):
        self.stream.close()

    def call(self, method, params):
        self.next += 1
        send(self.stream, {"jsonrpc": "2.0", "id": self.next, "method": method, "params": params})
        while True:
            message = receive(self.stream)
            if message.get("id") == self.next:
                require("error" not in message, f"RPC failed: {method}")
                return message["result"]
            self.pending.append(message)

    def mutate(self, method, **params):
        params.setdefault("idempotencyKey", str(uuid.uuid4()))
        return self.call(method, params)

    def snapshot(self):
        self.pending.clear()
        result = self.call("state.snapshot", {})
        parts = [p["params"] for p in self.pending if p.get("method") == "state.snapshot_part"]
        require(len(parts) == result["parts"], "incomplete snapshot")
        sections = {}
        for index, part in enumerate(parts):
            require(part["snapshotId"] == result["snapshotId"] and part["index"] == index
                    and part["of"] == len(parts), "inconsistent snapshot parts")
            sections.setdefault(part["section"], []).extend(part["data"])
        return result, sections

    def pane(self, pane_id):
        return next((p for p in self.snapshot()[1]["panes"] if p["paneId"] == pane_id), None)

    def replay(self, pane_id, origin="tail"):
        self.pending.clear()
        info = self.call("pane.stream.subscribe", {"paneId": pane_id, "from": origin})
        # Frames from an older mapping can precede the response to this request.
        messages = iter(self.pending)
        next_offset = None
        gaps = 0
        tail = bytearray()
        while True:
            message = next(messages, None)
            if message is None:
                message = receive(self.stream)
            if "bytes" in message:
                if message["binaryPaneRef"] != info["binaryPaneRef"]:
                    continue
                require(next_offset == message["offset"], "terminal output skipped or repeated bytes")
                require(message["generation"] == info["generation"], "terminal generation mismatch")
                next_offset += len(message["bytes"])
                tail.extend(message["bytes"])
                del tail[:-4096]
                continue
            params = message.get("params", {})
            if params.get("streamId") != info["streamId"]:
                continue
            if message.get("method") == "stream.replay_start":
                next_offset = int(params["fromOffset"])
                require(params["reset"] == (origin == "tail"), "wrong replay reset policy")
            elif message.get("method") == "stream.gap":
                require(next_offset == int(params["fromOffset"]), "gap starts at wrong offset")
                require(int(params["toOffset"]) > next_offset, "empty gap")
                next_offset = int(params["toOffset"])
                gaps += 1
            elif message.get("method") == "stream.replay_done":
                require(next_offset == int(params["nextOffset"]) == int(info["nextOffset"]),
                        "replay completion does not match its full interval")
                return info, bytes(tail), gaps


def programs_directory():
    """One executable directory per checkout, outside every privacy-protected folder.

    macOS privacy (TCC) records each executable path that asks for a protected
    resource. A fresh copy per run would add a new record every run, and a copy
    inside the checkout would itself sit in Documents when the checkout does.
    Runs stay isolated: each has its own data directory, sockets and launchd
    labels, and only these read-only executables are shared.
    """
    checkout = hashlib.sha256(str(ROOT).encode()).hexdigest()[:16]
    return Path.home() / "Library/Caches/soyeht-e2e" / checkout / "bin"


def stage_programs(binaries, names=PRODUCT_EXECUTABLES):
    directory = programs_directory()
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    for name in names:
        source, target = binaries / name, directory / name
        content = source.read_bytes()
        if target.is_file() and target.read_bytes() == content:
            continue
        # Replace by rename: a concurrent run keeps executing the old file.
        with tempfile.NamedTemporaryFile(dir=directory, prefix=f".{name}.", delete=False) as staged:
            staged.write(content)
        os.chmod(staged.name, 0o700)
        os.replace(staged.name, target)
    return directory


class Services:
    def __init__(self, binaries, engine="soyeht-engine"):
        identity = str(uuid.uuid4())
        # Keep Unix socket paths below the macOS sockaddr_un limit.
        self.data = short_temp_root() / f"sn-e2e-{identity}"
        self.data.mkdir(mode=0o700)
        self.data = self.data.resolve()
        shell_config=self.data/"shell-config"
        shell_config.mkdir(mode=0o700)
        (shell_config/".zshrc").write_text("PROMPT='E2E_READY> '\nRPROMPT=''\nHISTFILE=''\nSAVEHIST=0\n")
        # Per-run fakes and PATH entries; the product executables live in
        # `self.programs`, which is stable for this checkout.
        self.binaries=self.data / "bin"
        self.binaries.mkdir(mode=0o700)
        # Only macOS privacy (TCC) needs a stable copy; elsewhere the build's own are used.
        staged = (*PRODUCT_EXECUTABLES, executable(engine))
        self.programs = stage_programs(binaries, staged) if MACOS else binaries
        self.ptyd_version = int(subprocess.check_output(
            [str(self.programs / executable("soyeht-ptyd")), "--protocol-version"], text=True))
        self.launcher = make_launcher(self.data, f"{DEV_LAUNCHD_PREFIX}.e2e.{identity}", ROOT)
        self.clients = []
        self.definitions = {}
        for component in ("ptyd", "engine"):
            program = engine if component == "engine" else "soyeht-ptyd"
            argv = [self.programs / executable(program), "--data-dir", self.data]
            if component == "engine":
                argv.extend(["--listen", "127.0.0.1:0"])
            self.definitions[component] = (argv, {"ZDOTDIR": str(shell_config)})
            self.set_environment(component)

    def set_environment(self, component, **values):
        """Add environment variables to a service before it starts."""
        argv, environment = self.definitions[component]
        environment.update(values)
        self.launcher.define(component, argv, environment, self.data / f"{component}.log")

    def prepend_path(self, component, directory):
        """Put `directory` first on a service's PATH, ahead of the launcher's base PATH."""
        self.set_environment(component, PATH=f"{directory}{os.pathsep}{self.launcher.base_path}")

    def start(self, component):
        require(not self.launcher.exists(component), "test service already exists")
        self.launcher.start(component)

    def stop_engine(self):
        self.launcher.stop("engine")

    def client(self, name="acceptance", capabilities=None):
        deadline = time.monotonic() + 20
        while True:
            try:
                client = Client(self.data, name, capabilities)
                self.clients.append(client)
                return client
            except (OSError, EOFError):
                require(time.monotonic() < deadline, "engine readiness timeout")
                time.sleep(0.05)

    def restart_engine(self):
        client = self.client()
        generation = client.call("engine.status", {})["generation"]
        client.close()
        self.launcher.crash("engine")
        deadline = time.monotonic() + 20
        while True:
            try:
                client = self.client()
                if client.call("engine.status", {})["generation"] > generation:
                    return client
                client.close()
            except (OSError, EOFError):
                pass
            require(time.monotonic() < deadline, "the engine did not restart")
            time.sleep(0.05)

    def cli(self, *args):
        output = subprocess.run([str(self.programs / executable("soyeht")), "--profile", "dev",
                                 "--data-dir", str(self.data), *args], cwd=self.data,
                                capture_output=True, check=True)
        return json.loads(output.stdout)

    def close_supervised_processes(self):
        try:
            with connect_endpoint(self.data / DEV_PTYD_SOCKET, timeout=5) as stream:
                send(stream, {"method": "hello", "params": {"version": self.ptyd_version, "engine_generation": 18446744073709551615}})
                require(receive(stream)["status"] == "welcome", "cleanup supervisor handshake")
                send(stream, {"method": "list"})
                for instance in receive(stream)["value"]:
                    send(stream, {"method": "close", "params": {"instance_id": instance["instance_id"]}})
                    require(receive(stream)["status"] == "ok", "cleanup process close")
        except (OSError, EOFError):
            pass

    def close(self):
        for client in self.clients:
            client.close()
        if self.launcher.exists("engine"):
            self.launcher.stop("engine")
        mismatches = self.contract_mismatches()
        try:
            self.close_supervised_processes()
        finally:
            if self.launcher.exists("ptyd"):
                self.launcher.stop("ptyd")
        # Every service has exited by now, so no process holds a file of the folder.
        remove_directory(self.data)
        require(not mismatches, "the engine sent what the contract does not describe: " + "".join(mismatches[:1]))

    def contract_mismatches(self):
        """The engine, built with `contract-checks`, logs each event or result that breaks the contract."""
        # The engine rotates its log: `engine.log` and up to four older files beside it.
        logs = [self.data / "engine.log", *(self.data / f"engine.log.{n}" for n in range(1, 5))]
        lines = [line for log in logs if log.is_file() for line in log.read_text(errors="replace").splitlines()]
        return [line for line in lines if "engine.contract_mismatch" in line]


def wait_pane(client, pane_id):
    deadline = time.monotonic() + 15
    while True:
        pane = client.pane(pane_id)
        if pane and pane.get("pid") and b"E2E_READY>" in client.replay(pane_id)[1]:
            return pane
        require(time.monotonic() < deadline, "shell was not adopted")
        time.sleep(0.05)


def acceptances(services, results):
    created = services.cli("pane", "create", "--shell", "--idempotency-key", "acceptance-create")
    pane_id = created["paneId"]
    writer = services.client("writer")
    original = wait_pane(writer, pane_id)
    writer.mutate("pane.input", paneId=pane_id, bytes=base64.b64encode(b"printf 'ACCEPTANCE_SCREEN\\n'\n").decode())
    deadline = time.monotonic() + 15
    while True:
        _, tail, _ = writer.replay(pane_id)
        if b"ACCEPTANCE_SCREEN" in tail:
            break
        require(time.monotonic() < deadline, "shell output did not arrive")
        time.sleep(0.05)
    writer.close()
    writer = services.client("writer")
    require(wait_pane(writer, pane_id)["pid"] == original["pid"], "closing the client replaced the shell")
    require(b"ACCEPTANCE_SCREEN" in writer.replay(pane_id)[1], "reopened client lost its screen")
    results.append({"acceptance": 1, "name": "reopen_client", "passed": True})

    observer = services.restart_engine()
    restored = wait_pane(observer, pane_id)
    require(restored["pid"] == original["pid"] and restored["generation"] == original["generation"],
            "engine restart replaced its live shell")
    results.append({"acceptance": 2, "name": "launchd_adoption", "passed": True})

    observer.close()
    writer = services.client("writer")
    workspace = original["workspaceId"]
    writer.mutate("workspace.rename", workspaceId=workspace, name="While disconnected")
    observer = services.client("observer")
    snapshot, state = observer.snapshot()
    require(next(w for w in state["workspaces"] if w["workspaceId"] == workspace)["name"] == "While disconnected",
            "snapshot omitted a disconnected change")
    for index in range(4):
        writer.mutate("workspace.rename", workspaceId=workspace, name=f"Replay {index}")
    observer.pending.clear()
    observer.call("events.subscribe", {"since": snapshot["cursor"]})
    events = [m for m in observer.pending if m.get("method") == "workspace.renamed"]
    require([m["params"]["workspace"]["name"] for m in events] == [f"Replay {i}" for i in range(4)],
            "snapshot-to-subscription lost or duplicated a change")
    ids = [int(m["params"]["eventId"]) for m in events]
    require(ids == sorted(set(ids)) and ids[0] > int(snapshot["cursor"]), "event cursor is not exclusive and monotonic")
    writer.mutate("workspace.rename", workspaceId=workspace, name="Live")
    live = receive(observer.stream)
    require(live.get("method") == "workspace.renamed" and live["params"]["workspace"]["name"] == "Live"
            and int(live["params"]["eventId"]) > ids[-1], "live event did not follow replay")
    results.append({"acceptance": 3, "name": "snapshot_then_events", "passed": True})

    writer.mutate("pane.close", paneId=pane_id)
    closed = receive(observer.stream)
    require(closed.get("method") == "pane.closed" and closed["params"]["paneId"] == pane_id,
            "other client did not receive the close event")
    require(writer.pane(pane_id) is None and observer.pane(pane_id) is None, "closed pane remains visible")
    writer = services.restart_engine()
    require(writer.pane(pane_id) is None, "closed pane resurrected after restart")
    results.append({"acceptance": 4, "name": "close_is_durable", "passed": True})

    params = {"workspaceId": workspace, "mode": "terminal", "idempotencyKey": "duplicate-create"}
    first = writer.call("pane.create", params)
    require(writer.call("pane.create", params) == first, "duplicate request returned another pane")
    pane_id = first["paneId"]
    wait_pane(writer, pane_id)
    require(len(writer.snapshot()[1]["panes"]) == 1, "duplicate creation left multiple panes")
    registry = json.loads((services.data / "ptyd-registry.json").read_text())
    require(sum(not p["closed"] for p in registry["instances"].values()) == 1, "duplicate creation left multiple processes")
    results.append({"acceptance": 5, "name": "create_is_idempotent", "passed": True})

    before, _, _ = writer.replay(pane_id)
    command=b"sleep 0.2; head -c 9437184 /dev/zero; printf done > overflow.done\n"
    accepted=writer.mutate("pane.input", paneId=pane_id,bytes=base64.b64encode(command).decode())
    require(accepted["acceptedBytes"]==len(command),"overflow command was only partially accepted")
    services.stop_engine()
    deadline = time.monotonic() + 20
    while not (services.data / "overflow.done").exists():
        if time.monotonic() >= deadline:
            raise AssertionError("full supervisor buffer blocked the shell")
        time.sleep(0.05)
    services.start("engine")
    writer = services.client("writer")
    deadline = time.monotonic() + 20
    while True:
        _, state = writer.snapshot()
        stream = next(s for s in state["streams"] if s["paneId"] == pane_id)
        if int(stream["nextOffset"]) >= 9437184:
            break
        require(time.monotonic() < deadline, "retained output was not ingested")
        time.sleep(0.05)
    _, _, gaps = writer.replay(pane_id, {"streamId": before["streamId"], "offset": before["nextOffset"]})
    require(gaps > 0, "overflow did not produce an explicit gap")
    results.append({"acceptance": 6, "name": "overflow_reports_gap", "passed": True})
    writer.mutate("pane.close", paneId=pane_id)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "engine/target/debug")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    binaries = args.bin_dir.resolve()
    require(all((binaries / name).is_file() for name in
                PRODUCT_EXECUTABLES),
            "build workspace binaries before running acceptances")
    services = Services(binaries)
    started = time.monotonic()
    results=[]
    failure=None
    try:
        services.start("ptyd")
        services.start("engine")
        client = services.client()
        client.close()
        acceptances(services,results)
    except Exception as error:
        failure=str(error)
    finally:
        try:
            services.close()
        except Exception as error:
            failure=f"cleanup failed: {error}"
    report = {"passed":failure is None and len(results)==6, "seconds": round(time.monotonic() - started, 2), "results": results}
    if failure:report["error"]=failure
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if not report["passed"]:raise SystemExit(1)


if __name__ == "__main__":
    main()
