#!/usr/bin/env python3
"""Exercise the authenticated Claude and Codex ACP adapters in disposable local profiles."""

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


# The engine's registry snapshot pins the adapter versions the app launches.
REGISTRY = ROOT / "engine/soyeht-engine/src/agents/acp_registry_snapshot.json"
HARNESSES = {"claude": "registry.claude-acp", "codex": "registry.codex-acp"}
FULL_ACCESS = "full_access"


class NotSignedIn(Exception):
    """The adapter asked for sign-in; the harness is skipped, not failed."""


def raise_file_limit():
    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    ceiling = 4096 if hard == resource.RLIM_INFINITY else hard
    target = min(4096, ceiling)
    if soft < target:
        resource.setrlimit(resource.RLIMIT_NOFILE, (target, hard))


def pinned_package(registry_id):
    agents = json.loads(REGISTRY.read_text())["agents"]
    entry = next(agent for agent in agents if agent["id"] == registry_id)
    spec = next(arg for arg in entry["args"] if arg.startswith("@"))
    package, version = spec.rsplit("@", 1)
    return package, version


def install_adapter(directory, package, version):
    npm = shutil.which("npm")
    node = shutil.which("node")
    require(npm is not None and node is not None, "Node.js and npm are required")
    node_version = subprocess.check_output([node, "--version"], text=True).strip().lstrip("v")
    major = node_version.split(".", 1)[0]
    require(major.isdigit() and int(major) >= 22, "the real ACP adapters require Node.js 22 or later")
    subprocess.run([
        npm, "install", "--ignore-scripts", "--no-audit", "--no-fund",
        "--prefix", str(directory), f"{package}@{version}",
    ], check=True)
    adapter = directory / f"node_modules/{package}/dist/index.js"
    require(adapter.is_file(), f"npm did not install the pinned {package}")
    wrapper = directory / "run-real-acp"
    wrapper.write_text(
        "#!/bin/sh\nexec " + shlex.quote(node) + " " + shlex.quote(str(adapter)) + "\n"
    )
    wrapper.chmod(0o700)
    return wrapper


def agent_option(client, pane_id, predicate):
    return next(option for option in client.pane(pane_id)["agent"]["options"] if predicate(option))


def mode_option(client, pane_id):
    return agent_option(client, pane_id, lambda option: option.get("category") == "mode")


def set_mode(client, pane_id, mode):
    option = mode_option(client, pane_id)
    if option["currentValue"] != mode:
        applied = client.mutate(
            "pane.config.set", paneId=pane_id, configId=option["id"], value=mode
        )
        require(applied["effective"] == mode, f"the harness did not confirm mode {mode}")
    require(mode_option(client, pane_id)["currentValue"] == mode, f"mode {mode} is not shown")


def open_real_agent(services, name, adapter, version):
    if name == "claude":
        # A person's own Claude settings may allow every shell command in any
        # mode. This disposable project rule restores asking outside full access.
        settings = services.data / ".claude"
        settings.mkdir(mode=0o700)
        (settings / "settings.local.json").write_text(json.dumps({
            "permissions": {"ask": ["Bash(*)"]},
        }))
    (services.data / "harnesses.json").write_text(json.dumps([{
        "id": f"real-{name}",
        "name": f"Real {name}",
        "command": str(adapter),
        "args": [],
        "version": version,
    }]))
    services.start("ptyd")
    services.start("engine")
    client = services.client(f"real-{name}-mac")
    workspace = client.mutate(
        "workspace.create", name=f"Real {name} acceptance", path=str(services.data)
    )["workspace"]["workspaceId"]
    pane = client.mutate(
        "pane.create", workspaceId=workspace, mode="agent", harness=f"real-{name}"
    )["paneId"]

    def opened_or_needs_sign_in():
        agent = client.pane(pane).get("agent", {})
        return agent if agent.get("sessionId") or agent.get("authMethods") else None

    agent = until(opened_or_needs_sign_in, "the ACP session did not open", seconds=90)
    if not agent.get("sessionId"):
        raise NotSignedIn(f"{name} is not signed in on this Mac")
    return client, pane


def shell_turn(client, pane_id, target):
    # Outside the workspace: Codex's "Ask for approval" preset writes inside it
    # and in temporary directories without asking, and asks for other files.
    return client.mutate(
        "pane.prompt",
        paneId=pane_id,
        text=f"Use the shell to run: printf SAFE > {target}. If a sandbox blocks it, "
             "request approval to run it outside the sandbox. Then reply DONE.",
    )["promptId"]


def signed_in(client, pane_id):
    """A turn that fails authentication makes the engine ask for sign-in again."""
    if client.pane(pane_id)["agent"].get("authMethods"):
        raise NotSignedIn("the harness's sign-in expired during the acceptance")
    return True


def approval_mode_asks(services, mac, phone, pane_id, approval, outside):
    set_mode(phone, pane_id, approval)
    target = outside / "approval-check.txt"
    prompt_id = shell_turn(mac, pane_id, target)
    pending = until(
        lambda: signed_in(phone, pane_id) and question(phone, pane_id),
        f"mode {approval} did not forward the shell permission",
        seconds=180,
    )
    allow = next(
        option["optionId"] for option in pending["options"] if option["kind"] == "allow_once"
    )
    phone.mutate(
        "pane.permission.answer", paneId=pane_id, requestId=pending["requestId"], optionId=allow
    )
    until(lambda: ended(services, prompt_id), "permission turn did not end", seconds=180)
    require(target.read_text() == "SAFE", "the approved harmless action did not complete")


def full_access_runs_without_asking(services, mac, pane_id, full_access, approval, outside):
    count = len(events(services, "pane.permission_requested", pane_id))
    set_mode(mac, pane_id, full_access)
    target = outside / "full-access-check.txt"
    prompt_id = shell_turn(mac, pane_id, target)
    until(lambda: ended(services, prompt_id), "full-access turn did not end", seconds=180)
    require(
        target.read_text() == "SAFE"
        and len(events(services, "pane.permission_requested", pane_id)) == count,
        "the harness's full-access mode still asked",
    )
    set_mode(mac, pane_id, approval)


def model_change_preserves_session(mac, pane_id):
    session_id = mac.pane(pane_id)["agent"]["sessionId"]
    model = agent_option(mac, pane_id, lambda option: option.get("category") == "model")
    original = model["currentValue"]
    alternative = next(
        option["value"] for option in model["options"] if option["value"] != original
    )
    changed = mac.mutate(
        "pane.config.set", paneId=pane_id, configId=model["id"], value=alternative
    )
    require(changed["sessionPreserved"], "model change replaced the real session")
    restored = mac.mutate(
        "pane.config.set", paneId=pane_id, configId=model["id"], value=original
    )
    require(
        restored["sessionPreserved"] and mac.pane(pane_id)["agent"]["sessionId"] == session_id,
        "restoring the model replaced the real session",
    )


def engine_restart_preserves_process(services, mac, pane_id):
    before = mac.pane(pane_id)
    restart_prompt = mac.mutate(
        "pane.prompt",
        paneId=pane_id,
        text="Write the integers 1 through 200, one per line, and nothing else.",
    )["promptId"]
    until(lambda: mac.pane(pane_id)["state"] == "working",
          "the restart prompt did not begin", seconds=30)
    mac = services.restart_engine()
    require(mac.pane(pane_id)["pid"] == before["pid"], "engine restart replaced the ACP process")
    until(lambda: ended(services, restart_prompt),
          "the real turn did not finish after engine restart", seconds=240)
    require(mac.pane(pane_id)["state"] == "idle", "the real pane did not return to idle")
    transcript = mac.call("pane.transcript", {"paneId": pane_id, "limit": 100})["items"]
    require(any(item["role"] == "assistant" and item["kind"] == "message"
                and item["complete"] for item in transcript),
            "the recovered real turn has no complete assistant message")


def transcript_tail(client, pane_id):
    items = client.call("pane.transcript", {"paneId": pane_id, "limit": 12})["items"]
    return [{"role": item["role"], "kind": item["kind"], "text": item.get("text", "")[:300],
             "tool": json.dumps(item.get("tool"))[:600] if item.get("tool") else None}
            for item in items]


def run_harness(binaries, name, adapter, version, report, model=None):
    services = Services(binaries)
    # Harness sandboxes let shell commands write the workspace and temporary
    # directories; a disposable directory under the user cache is outside both.
    outside = Path(tempfile.mkdtemp(
        prefix=f"soyeht-real-{name}-outside-", dir=Path.home() / "Library/Caches")).resolve()
    mac = pane_id = None
    try:
        mac, pane_id = open_real_agent(services, name, adapter, version)
        phone = services.client(f"real-{name}-second-client")
        if model:
            option = agent_option(mac, pane_id, lambda option: option.get("category") == "model")
            mac.mutate("pane.config.set", paneId=pane_id, configId=option["id"], value=model)
            report["model"] = model
        option = mode_option(mac, pane_id)
        modes = option["options"]
        report["modes"] = [
            {"id": mode["value"], "name": mode["name"], "kind": mode.get("kind")}
            for mode in modes
        ]
        report["initialMode"] = option["currentValue"]
        results = report["results"]
        results.append({"name": "real_modes_listed", "passed": bool(modes)})
        require(modes, f"{name} announced no permission modes")
        # Adapters announce their most restrictive mode first.
        approval = modes[0]["value"]
        full_access = next(
            (mode["value"] for mode in modes if mode.get("kind") == FULL_ACCESS), None
        )
        approval_mode_asks(services, mac, phone, pane_id, approval, outside)
        results.append({"name": "real_approval_mode_asks_for_shell", "mode": approval,
                        "passed": True})
        if full_access:
            full_access_runs_without_asking(
                services, mac, pane_id, full_access, approval, outside)
            results.append({"name": "real_full_access_mode_runs_without_asking",
                            "mode": full_access, "passed": True})
        model_change_preserves_session(mac, pane_id)
        results.append({"name": "real_model_change_preserves_session", "passed": True})
        engine_restart_preserves_process(services, mac, pane_id)
        results.append({"name": "real_engine_restart_preserves_process", "passed": True})
        report["status"] = "passed"
    except (AssertionError, NotSignedIn):
        if pane_id:
            report["transcriptTail"] = transcript_tail(services.client(), pane_id)
        raise
    finally:
        shutil.rmtree(outside, ignore_errors=True)
        services.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "engine/target/debug")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--harness", choices=sorted(HARNESSES), action="append")
    parser.add_argument(
        "--model", action="append", default=[], metavar="HARNESS=MODEL",
        help="start a harness on one of its announced models, e.g. when the account "
             "cannot use the configured default")
    args = parser.parse_args()
    if os.environ.get("SOYEHT_REAL_ACP") != "1":
        raise SystemExit("Set SOYEHT_REAL_ACP=1 to authorize the authenticated local test")
    raise_file_limit()
    binaries = args.bin_dir.resolve()
    require(all((binaries / name).is_file() for name in
                ("soyeht", "soyeht-engine", "soyeht-ptyd")),
            "build workspace binaries before running the real ACP acceptance")
    models = dict(value.split("=", 1) for value in args.model)
    started = time.monotonic()
    harnesses = {}
    for name in args.harness or sorted(HARNESSES):
        package, version = pinned_package(HARNESSES[name])
        report = harnesses[name] = {
            "adapter": package, "version": version, "status": "failed", "results": [],
        }
        try:
            with tempfile.TemporaryDirectory(prefix=f"soyeht-real-{name}-") as directory:
                adapter = install_adapter(Path(directory), package, version)
                run_harness(binaries, name, adapter, version, report, models.get(name))
        except NotSignedIn as reason:
            report["status"] = "skipped"
            report["reason"] = str(reason)
        except Exception as error:
            report["error"] = f"{type(error).__name__}: {error}"
    summary = {
        "passed": all(report["status"] != "failed" for report in harnesses.values()),
        "skipped": sorted(name for name, report in harnesses.items()
                          if report["status"] == "skipped"),
        "seconds": round(time.monotonic() - started, 2),
        "harnesses": harnesses,
    }
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    if not summary["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
