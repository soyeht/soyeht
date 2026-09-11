#!/usr/bin/env python3
"""Exercise the authenticated ACP adapter in a disposable local profile."""

import argparse
import json
import os
import resource
import shlex
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from e2e import ROOT, Services, require
from e2e_agents import ended, events, question, until


ADAPTER_PACKAGE = "@agentclientprotocol/claude-agent-acp"
ADAPTER_VERSION = "0.76.0"


def raise_file_limit():
    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    ceiling = 4096 if hard == resource.RLIM_INFINITY else hard
    target = min(4096, ceiling)
    if soft < target:
        resource.setrlimit(resource.RLIMIT_NOFILE, (target, hard))


def install_adapter(directory):
    npm = shutil.which("npm")
    node = shutil.which("node")
    require(npm is not None and node is not None, "Node.js and npm are required")
    version = subprocess.check_output([node, "--version"], text=True).strip().lstrip("v")
    require(version.split(".", 1)[0].isdigit() and int(version.split(".", 1)[0]) >= 22,
            "the real ACP adapter requires Node.js 22 or later")
    subprocess.run([
        npm, "install", "--ignore-scripts", "--no-audit", "--no-fund",
        "--prefix", str(directory), f"{ADAPTER_PACKAGE}@{ADAPTER_VERSION}",
    ], check=True)
    adapter = directory / f"node_modules/{ADAPTER_PACKAGE}/dist/index.js"
    require(adapter.is_file(), "npm did not install the pinned adapter")
    wrapper = directory / "run-real-acp"
    wrapper.write_text(
        "#!/bin/sh\nexec " + shlex.quote(node) + " " + shlex.quote(str(adapter)) + "\n"
    )
    wrapper.chmod(0o700)
    return wrapper


def agent_option(client, pane_id, option_id):
    pane = client.pane(pane_id)
    return next(option for option in pane["agent"]["options"] if option["id"] == option_id)


def run_acceptances(binaries, adapter):
    services = Services(binaries)
    results = []
    try:
        settings = services.data / ".claude"
        settings.mkdir(mode=0o700)
        (settings / "settings.local.json").write_text(json.dumps({
            "permissions": {"ask": ["Bash(*)"]},
        }))
        (services.data / "harnesses.json").write_text(json.dumps([{
            "id": "real-acp",
            "name": "Real ACP",
            "command": str(adapter),
            "args": [],
            "version": ADAPTER_VERSION,
        }]))
        services.start("ptyd")
        services.start("engine")
        mac = services.client("real-acp-mac")
        workspace = mac.mutate(
            "workspace.create", name="Real ACP acceptance", path=str(services.data)
        )["workspace"]["workspaceId"]
        pane_id = mac.mutate(
            "pane.create", workspaceId=workspace, mode="agent", harness="real-acp"
        )["paneId"]
        until(
            lambda: mac.pane(pane_id).get("agent", {}).get("sessionId"),
            "the authenticated ACP session did not open",
            seconds=90,
        )

        phone = services.client("real-acp-second-client")
        prompt_id = mac.mutate(
            "pane.prompt",
            paneId=pane_id,
            text="Use the shell to run: printf SAFE > permission-check.txt. Then reply DONE.",
        )["promptId"]
        pending = until(
            lambda: question(phone, pane_id),
            "the adapter did not forward the forced harmless permission",
            seconds=180,
        )
        allow = next(
            option["optionId"] for option in pending["options"]
            if option["kind"] == "allow_once"
        )
        phone.mutate(
            "pane.permission.answer",
            paneId=pane_id,
            requestId=pending["requestId"],
            optionId=allow,
        )
        until(lambda: ended(services, prompt_id), "permission turn did not end", seconds=180)
        require(
            (services.data / "permission-check.txt").read_text() == "SAFE",
            "the approved harmless action did not complete",
        )
        results.append({"name": "real_prompt_and_permission_bridge", "passed": True})

        session_id = mac.pane(pane_id)["agent"]["sessionId"]
        model = agent_option(mac, pane_id, "model")
        original = model["currentValue"]
        alternative = next(
            option["value"] for option in model["options"]
            if option["value"] != original
        )
        changed = mac.mutate(
            "pane.config.set", paneId=pane_id, configId="model", value=alternative
        )
        require(changed["sessionPreserved"], "model change replaced the real session")
        restored = mac.mutate(
            "pane.config.set", paneId=pane_id, configId="model", value=original
        )
        require(
            restored["sessionPreserved"]
            and mac.pane(pane_id)["agent"]["sessionId"] == session_id,
            "restoring the model replaced the real session",
        )
        results.append({"name": "real_model_change_preserves_session", "passed": True})

        count = len(events(services, "pane.permission_requested", pane_id))
        require(mac.mutate("pane.yolo", paneId=pane_id, on=True)["on"],
                "yolo did not turn on")
        yolo_prompt = mac.mutate(
            "pane.prompt",
            paneId=pane_id,
            text="Use the shell to run: printf SAFE > yolo-check.txt. Then reply DONE.",
        )["promptId"]
        until(lambda: ended(services, yolo_prompt), "yolo turn did not end", seconds=180)
        require(
            (services.data / "yolo-check.txt").read_text() == "SAFE"
            and len(events(services, "pane.permission_requested", pane_id)) == count,
            "yolo did not keep the real permission request out of the user surface",
        )
        require(not mac.mutate("pane.yolo", paneId=pane_id, on=False)["on"],
                "yolo did not turn off")
        results.append({"name": "real_yolo_round_trip", "passed": True})

        before = mac.pane(pane_id)
        restart_prompt = mac.mutate(
            "pane.prompt",
            paneId=pane_id,
            text="Write the integers 1 through 200, one per line, and nothing else.",
        )["promptId"]
        until(lambda: mac.pane(pane_id)["state"] == "working",
              "the restart prompt did not begin", seconds=30)
        mac = services.restart_engine()
        require(mac.pane(pane_id)["pid"] == before["pid"],
                "engine restart replaced the real ACP process")
        until(lambda: ended(services, restart_prompt),
              "the real turn did not finish after engine restart", seconds=240)
        require(mac.pane(pane_id)["state"] == "idle",
                "the real pane did not return to idle")
        transcript = mac.call("pane.transcript", {"paneId": pane_id, "limit": 100})["items"]
        require(any(item["role"] == "assistant" and item["kind"] == "message"
                    and item["complete"] for item in transcript),
                "the recovered real turn has no complete assistant message")
        results.append({"name": "real_engine_restart_preserves_process", "passed": True})
        return results
    finally:
        services.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "engine/target/debug")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if os.environ.get("SOYEHT_REAL_ACP") != "1":
        raise SystemExit("Set SOYEHT_REAL_ACP=1 to authorize the authenticated local test")
    raise_file_limit()
    binaries = args.bin_dir.resolve()
    require(all((binaries / name).is_file() for name in
                ("soyeht", "soyeht-engine", "soyeht-ptyd")),
            "build workspace binaries before running the real ACP acceptance")
    started = time.monotonic()
    results = []
    failure = None
    try:
        with tempfile.TemporaryDirectory(prefix="soyeht-real-acp-") as directory:
            adapter = install_adapter(Path(directory))
            results = run_acceptances(binaries, adapter)
    except Exception as error:
        failure = f"{type(error).__name__}: {error}"
    report = {
        "passed": failure is None and len(results) == 4,
        "adapterVersion": ADAPTER_VERSION,
        "seconds": round(time.monotonic() - started, 2),
        "results": results,
    }
    if failure:
        report["error"] = failure
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if not report["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
