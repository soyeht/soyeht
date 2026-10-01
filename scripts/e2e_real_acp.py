#!/usr/bin/env python3
"""Exercise the authenticated Claude and Codex ACP adapters in disposable local profiles."""

import argparse
import json
import os
import platform
import resource
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from e2e import ROOT, Services, require
from e2e_agents import ended, events, question, until


# The engine's registry snapshot pins the adapter versions the app launches.
REGISTRY = ROOT / "engine/soyeht-engine/src/agents/acp_registry_snapshot.json"
HARNESSES = {"claude": "registry.claude-acp", "codex": "registry.codex-acp"}
FULL_ACCESS = "full_access"
# A person's own agent extensions (MCP servers, hooks, plugins) often run from
# other apps' bundles or from privacy-protected folders. Started under the
# disposable supervisor, they make macOS privacy prompts name that supervisor.
# The real harnesses therefore run with only the MCP server the engine declares.
CODEX_FEATURES_OFF = ("apps", "browser_use", "computer_use", "hooks", "plugins")
CODEX_PROJECT = "codex-project.toml"


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
    directory.mkdir(mode=0o700, exist_ok=True)
    subprocess.run([
        npm, "install", "--ignore-scripts", "--no-audit", "--no-fund",
        "--prefix", str(directory), f"{package}@{version}",
    ], check=True)
    adapter = directory / f"node_modules/{package}/dist/index.js"
    require(adapter.is_file(), f"npm did not install the pinned {package}")
    environment = quiet_launcher(directory, package, node)
    return write_script(directory / "run-real-acp", [
        *(f"export {key}={shlex.quote(value)}" for key, value in environment.items()),
        f"exec {shlex.quote(node)} {shlex.quote(str(adapter))}",
    ])


def write_script(path, lines):
    path.write_text("#!/bin/sh\n" + "\n".join(lines) + "\n")
    path.chmod(0o700)
    return path


def installed_file(directory, package, relative):
    # npm hoists a dependency unless another version already holds its place.
    candidates = (directory / "node_modules" / relative,
                  directory / "node_modules" / package / "node_modules" / relative)
    found = next((path for path in candidates if path.is_file()), None)
    require(found is not None, f"npm did not install {relative} for {package}")
    return found


def quiet_launcher(directory, package, node):
    """Environment that makes the adapter start its CLI without user extensions."""
    if package.endswith("/claude-agent-acp"):
        arch = {"x86_64": "x64", "aarch64": "arm64"}.get(platform.machine(), platform.machine())
        claude = installed_file(
            directory, package, f"@anthropic-ai/claude-agent-sdk-{sys.platform}-{arch}/claude")
        shim = write_script(directory / "claude-quiet", [
            f'exec {shlex.quote(str(claude))} --strict-mcp-config "$@"'])
        return {"CLAUDE_CODE_EXECUTABLE": str(shim)}
    codex = installed_file(directory, package, "@openai/codex/bin/codex.js")
    features = [item for name in CODEX_FEATURES_OFF for item in ("-c", f"features.{name}=false")]
    listing = subprocess.run(
        [node, str(codex), *features, "mcp", "list", "--json"],
        cwd=directory, check=True, capture_output=True, text=True, timeout=60,
    )
    names = [server["name"] for server in json.loads(listing.stdout)]
    require(all(name.replace("-", "").replace("_", "").isalnum() for name in names),
            "a configured Codex MCP server name cannot be disabled by override")
    # A session that declares MCP servers replaces the command-line MCP
    # overrides but still merges its trusted project configuration; sessions
    # that declare none (titles, file-change reports) keep the command-line overrides.
    (directory / CODEX_PROJECT).write_text("".join(
        f"[mcp_servers.{name}]\nenabled = false\n\n" for name in names))
    disabled = [item for name in names for item in ("-c", f"mcp_servers.{name}.enabled=false")]
    shim = write_script(directory / "codex-quiet", [
        "exec " + shlex.join([node, str(codex), *features, *disabled]) + ' "$@"'])
    return {"CODEX_PATH": str(shim)}


def quiet_workspace(workspace, adapters, claude_settings=None):
    """Project configuration that outranks the person's own for both harnesses."""
    user = Path(os.environ.get("CLAUDE_CONFIG_DIR") or Path.home() / ".claude")
    try:
        plugins = json.loads((user / "settings.json").read_text()).get("enabledPlugins") or {}
    except (OSError, ValueError, AttributeError):
        plugins = {}
    claude = workspace / ".claude"
    claude.mkdir(mode=0o700, exist_ok=True)
    (claude / "settings.local.json").write_text(json.dumps({
        **(claude_settings or {}),
        "disableAllHooks": True,
        "enabledPlugins": {name: False for name in plugins},
    }))
    for adapter in adapters:
        project = adapter.parent / CODEX_PROJECT
        if project.is_file():
            codex = workspace / ".codex"
            codex.mkdir(mode=0o700, exist_ok=True)
            shutil.copyfile(project, codex / "config.toml")


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
    # A person's own Claude settings may allow every shell command in any
    # mode. This disposable project rule restores asking outside full access.
    quiet_workspace(services.data, [adapter], {"permissions": {"ask": ["Bash(*)"]}})
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


SUBAGENT_PROMPT = (
    "Spawn exactly two subagents in parallel. Without using any tools, the first must "
    "reply with the single word ALPHA and the second with the single word BETA. Wait for "
    "both, then reply DONE."
)


def subagents_are_grouped(services, mac, pane_id, name):
    """Each subagent is one completed row that owns its activity."""
    prompt_id = mac.mutate("pane.prompt", paneId=pane_id, text=SUBAGENT_PROMPT)["promptId"]
    until(lambda: ended(services, prompt_id), "the subagent turn did not finish", seconds=300)
    items = mac.call("pane.transcript", {"paneId": pane_id, "limit": 100})["items"]
    rows = [item["subagent"] for item in items if item["kind"] == "subagent"]
    require(len(rows) >= 2, f"{name} reported {len(rows)} subagent rows, expected 2")
    for run in rows:
        owned = [item for item in items if item.get("subagentId") == run["subagentId"]]
        require(run["status"] == "completed", f"subagent {run['name']} ended {run['status']}")
        require(owned, f"subagent {run['name']} owns no activity")
    # Claude's spawning Agent calls report into the rows, never as parent tool rows.
    require(not any(item["kind"] == "tool" and not item.get("subagentId")
                    and item["tool"].get("title") in ("Agent", "Task") for item in items),
            "a spawning Agent call appears as the parent's own tool call")
    return [{"name": run["name"], "model": run.get("model"),
             "owned": sum(item.get("subagentId") == run["subagentId"] for item in items)}
            for run in rows]


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
        subagents = subagents_are_grouped(services, mac, pane_id, name)
        results.append({"name": "real_subagents_are_grouped", "subagents": subagents,
                        "passed": True})
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
