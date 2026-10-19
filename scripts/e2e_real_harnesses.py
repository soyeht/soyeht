#!/usr/bin/env python3
"""Opt-in acceptance of the real Claude Code and Codex adapters on macOS, Linux and Windows.

One disposable engine runs both pinned registry adapters and proves, with tiny prompts:

1. each signed-in harness answers (the model it used is reported);
2. a pane switches Claude -> Codex and back, answers on each side, and shows the handover;
3. Codex -> Claude and Claude -> Codex messages through the Soyeht MCP tools, each
   answered by the recipient and shown in the sender's transcript.

A harness that is not signed in is skipped by name, and the steps that need it are skipped;
nothing fails because of a missing sign-in. Set SOYEHT_REAL_ACP=1 to run it.
"""

import os
import subprocess
import tempfile
import time
from pathlib import Path

from e2e import (
    PRODUCT_NAMES,
    ROOT,
    Services,
    argument_parser,
    describe_failure,
    finish_timed_report,
    require,
    require_binaries,
)
from e2e_agents import assistant_text, ended, transcript, turn_items
from e2e_real_acp import HARNESSES, authorize_real_run, quiet_workspace
from e2e_real_agent_messaging import (
    Run,
    SignedOut,
    exchange,
    print_transcripts,
    register_adapters,
)

SWITCH_SECONDS = 240
REMEMBERED = "orchid"
# Harness ids in the disposable registry, which `register_adapters` names real-<key>.
HARNESSES_BY_KEY = {key: f"real-{key}" for key in HARNESSES}


def current_model(client, pane):
    option = next((item for item in client.pane(pane)["agent"]["options"]
                   if item.get("category") == "model"), None)
    return option["currentValue"] if option else None


def ask(run, agent, text, expect):
    """One tiny turn on `agent`; its reply must contain `expect`."""
    prompt = run.client.mutate("pane.prompt", paneId=agent.pane, text=text)["promptId"]
    run.until(lambda: ended(run.services, prompt), f"{agent.key} did not finish its turn")
    reply = assistant_text(turn_items(run.client, agent.pane, prompt))
    require(expect in reply.lower(), f"{agent.key} replied {reply[:200]!r}, expected {expect!r}")
    return reply


def handovers(run, agent):
    return [item for item in transcript(run.client, agent.pane) if item["kind"] == "handover"]


def switch(run, agent, target):
    """Switch the pane to harness `target`; it answers, remembers, and shows the handover."""
    before = len(handovers(run, agent))
    # The call returns when the new session is ready, which can take longer than a request.
    run.client.stream.settimeout(SWITCH_SECONDS)
    try:
        result = run.client.mutate("pane.harness.switch", paneId=agent.pane,
                                   harness=HARNESSES_BY_KEY[target])
    finally:
        run.client.stream.settimeout(10)
    require(result["toHarness"] == HARNESSES_BY_KEY[target], "the switch went elsewhere")
    require(run.client.pane(agent.pane)["agent"]["harness"] == HARNESSES_BY_KEY[target],
            "the pane does not show the new harness")
    agent.key = target
    run.until(lambda: len(handovers(run, agent)) > before, "the transcript shows no switch")
    reply = ask(run, agent, "What word did I ask you to remember? Reply with that word only.",
                REMEMBERED)
    run.note(f"switched to {target}; it answered {reply[:60]!r} and the transcript shows the switch")
    return current_model(run.client, agent.pane)


def open_signed_in(run, workspace, keys, models, report):
    opened = {}
    for key in keys:
        try:
            agent = run.open(workspace, key, models.get(key))
        except SignedOut:
            report["harnesses"][key] = {"status": "skipped", "reason": "not signed in"}
            run.note(f"{key}: skipped, not signed in")
            continue
        opened[key] = agent
        report["harnesses"][key] = {"status": "passed",
                                    "model": current_model(run.client, agent.pane)}
    return opened


def scenarios(run, workspace, models, report):
    opened = open_signed_in(run, workspace, sorted(HARNESSES), models, report)
    steps = report["steps"]
    for key, agent in opened.items():
        reply = ask(run, agent, "Reply with the single word pong.", "pong")
        steps[f"{key}_replies"] = {"reply": reply[:80]}
    if len(opened) < len(HARNESSES):
        for name in ("switch_both_ways", "codex_messages_claude", "claude_messages_codex"):
            steps[name] = {"status": "skipped", "reason": "needs both harnesses signed in"}
        return
    exchange(run, opened["codex"], opened["claude"])
    steps["codex_messages_claude"] = {"status": "passed"}
    exchange(run, opened["claude"], opened["codex"])
    steps["claude_messages_codex"] = {"status": "passed"}
    # A pane of its own for the switch, so the messaging panes keep their harness.
    switcher = run.open(workspace, "claude", models.get("claude"))
    ask(run, switcher, f"Remember the word {REMEMBERED}. Reply with the single word pong.", "pong")
    steps["switch_both_ways"] = {
        "codexModel": switch(run, switcher, "codex"),
        "claudeModel": switch(run, switcher, "claude"),
    }


def run_real(binaries, models, report):
    with tempfile.TemporaryDirectory(prefix="soyeht-real-", ignore_cleanup_errors=True) as directory:
        services = Services(binaries)
        run = None
        try:
            adapters = register_adapters(services, sorted(HARNESSES), Path(directory))
            services.start("ptyd")
            services.start("engine")
            client = services.client("real-harnesses")
            project = services.data / "project"
            project.mkdir()
            quiet_workspace(project, adapters)
            workspace = client.mutate("workspace.create", name="Real harnesses",
                                      path=str(project))["workspace"]["workspaceId"]
            run = Run(services, client)
            scenarios(run, workspace, models, report)
            report["log"] = run.log
        except SignedOut as error:
            # A sign-in that expires mid-run ends the run without failing it.
            report["skipped"] = str(error)
        except Exception:
            print_transcripts(run)
            raise
        finally:
            services.close()


def build_binaries():
    """The programs the acceptance starts, from this checkout (incremental after the first run)."""
    packages = ("soyeht-engine", "soyeht-ptyd", "soyeht-cli", "soyeht-mcp")
    subprocess.run(["cargo", "build", "-j", os.environ.get("SOYEHT_JOBS", "4"),
                    *(arg for package in packages for arg in ("-p", package))],
                   cwd=ROOT / "engine", check=True)


def main():
    parser = argument_parser(
        __doc__, Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "engine/target")) / "debug")
    parser.add_argument("--build", action="store_true",
                        help="build the engine programs from this checkout first (needs cargo)")
    parser.add_argument("--model", action="append", default=[], metavar="HARNESS=MODEL",
                        help="select a model option for a harness, such as codex=gpt-5.5")
    args = parser.parse_args()
    authorize_real_run()
    if args.build:
        build_binaries()
    binaries = args.bin_dir.resolve()
    require_binaries(binaries, PRODUCT_NAMES,
                     "build workspace binaries before running the real harness acceptance")
    started = time.monotonic()
    report = {"harnesses": {}, "steps": {}}
    try:
        run_real(binaries, dict(choice.split("=", 1) for choice in args.model), report)
    except Exception as error:
        report["error"] = describe_failure(error)
    finish_timed_report(report, started, args.report)


if __name__ == "__main__":
    main()
