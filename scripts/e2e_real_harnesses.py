#!/usr/bin/env python3
"""Opt-in acceptance of the four supported agent harnesses on macOS, Linux and Windows.

One disposable engine runs the harnesses the product ships against: the two pinned registry
adapters (`claude`, `codex`) and the two native ACP commands of the built-in registry
(`cursor` is `cursor-agent acp`, `opencode` is `opencode acp`), each launched as the app
launches it. With tiny prompts it proves, per harness:

1. a session opens and a streamed reply arrives (the model it used is reported);
2. the model, effort and permission selectors the harness exposes can be changed and restored
   without replacing the session;
3. a running turn is stopped and the pane answers the next prompt;
4. an engine restart during a turn keeps the harness process and the turn finishes.

and across the harnesses that are available:

5. each harness messages the next one through the Soyeht MCP tools and is answered, both
   shown in the transcripts;
6. one pane switches through every harness and back, answers on each side, and shows the
   handovers.

A harness whose CLI is missing or that is not signed in is SKIPPED with the reason, and the
steps that need it are left out; nothing is reported as passed without having run. Set
SOYEHT_REAL_ACP=1 to run it; `--harness` selects some of the four.
"""

import json
import os
import shutil
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
from e2e_agents import assistant_text, ended, events, transcript, turn_items
from e2e_real_acp import (
    ALL_HARNESSES,
    HARNESSES,
    authorize_real_run,
    harness_id,
    quiet_workspace,
)
from e2e_real_agent_messaging import (
    Run,
    SignedOut,
    exchange,
    print_transcripts,
    register_adapters,
)

SWITCH_SECONDS = 240
CANCEL_SECONDS = 60
REMEMBERED = "orchid"
LONG_PROMPT = "Write the integers 1 through 400, one per line, and nothing else."
# The selectors of x-clientPolicy `configRoles`: a role is named by its category, or by its id.
ROLES = {"model": ("model", "model"), "effort": ("thought_level", "effort"),
         "permission": ("mode", "mode")}
FULL_ACCESS = "full_access"


def role_option(client, pane, role):
    category, legacy = ROLES[role]
    return next((item for item in client.pane(pane)["agent"]["options"]
                 if item.get("category") == category or item["id"] == legacy), None)


def current_model(client, pane):
    option = role_option(client, pane, "model")
    return option["currentValue"] if option else None


def ask(run, agent, text, expect):
    """One tiny turn on `agent`; its reply must contain `expect`."""
    prompt = run.client.mutate("pane.prompt", paneId=agent.pane, text=text)["promptId"]
    run.until(lambda: ended(run.services, prompt), f"{agent.key} did not finish its turn")
    items = turn_items(run.client, agent.pane, prompt)
    if notice := provider_error(run, agent, prompt, items):
        raise ProviderError(agent.key, notice)
    reply = assistant_text(items)
    require(expect in reply.lower(), f"{agent.key} replied {reply[:200]!r}, expected {expect!r}")
    return reply


def reply_streams(run, agent):
    """A reply that arrives, and whose text reached clients as updates before the turn ended."""
    reply = ask(run, agent, "Reply with the single word pong.", "pong")
    updates = events(run.services, "pane.update", agent.pane)
    require(updates, f"{agent.key} sent no streamed update for its reply")
    return {"reply": reply[:80], "updates": len(updates)}


def selector_roundtrip(run, agent, role):
    """Change a selector to another value and back; the session must survive both changes."""
    client = run.client
    option = role_option(client, agent.pane, role)
    if option is None or len(option["options"]) < 2:
        return {"status": "skipped", "reason": f"the harness exposes no {role} choice"}
    session = client.pane(agent.pane)["agent"]["sessionId"]
    original = option["currentValue"]
    # Full access can run commands unasked; a probe of the permission selector avoids it.
    alternatives = [value for value in option["options"]
                    if value["value"] != original and value.get("kind") != FULL_ACCESS]
    if not alternatives:
        return {"status": "skipped", "reason": f"the harness offers no other {role} value"}
    for value in (alternatives[0]["value"], original):
        applied = client.mutate("pane.config.set", paneId=agent.pane, configId=option["id"],
                                value=value)
        require(applied["effective"] == value, f"{agent.key} did not confirm {role} {value!r}")
        require(applied["sessionPreserved"], f"{agent.key} replaced its session for {role}")
    require(client.pane(agent.pane)["agent"]["sessionId"] == session,
            f"{agent.key} changed session after the {role} round trip")
    return {"status": "passed", "from": original, "to": alternatives[0]["value"]}


def selectors(run, agent):
    return {role: selector_roundtrip(run, agent, role) for role in ROLES}


def stop_turn(run, agent):
    """Cancel a running turn: the process survives, and the pane answers the next prompt."""
    client = run.client
    pid = client.pane(agent.pane)["pid"]
    prompt = client.mutate("pane.prompt", paneId=agent.pane, text=LONG_PROMPT)["promptId"]
    run.until(lambda: client.pane(agent.pane)["state"] == "working", "the turn did not begin",
              seconds=60)
    client.mutate("pane.cancel", paneId=agent.pane)
    run.until(lambda: client.pane(agent.pane)["state"] == "idle", "cancel was not confirmed",
              seconds=CANCEL_SECONDS)
    require(ended(run.services, prompt), f"{agent.key}'s cancelled turn never ended")
    require(client.pane(agent.pane)["pid"] == pid, f"{agent.key} was restarted by the cancel")
    if client.pane(agent.pane)["agent"].get("queuePaused"):
        client.mutate("pane.queue.resume", paneId=agent.pane)
    reply = ask(run, agent, "Reply with the single word pong.", "pong")
    return {"reply": reply[:80]}


def restart_during_turns(run, opened):
    """Kill the engine while every pane streams a turn; processes and turns survive it."""
    prompts = {key: run.client.mutate("pane.prompt", paneId=agent.pane,
                                      text=LONG_PROMPT)["promptId"]
               for key, agent in opened.items()}
    for agent in opened.values():
        run.until(lambda: run.client.pane(agent.pane)["state"] == "working",
                  f"{agent.key}'s turn did not begin", seconds=60)
    pids = {key: run.client.pane(agent.pane)["pid"] for key, agent in opened.items()}
    run.client = run.services.restart_engine()
    for key, agent in opened.items():
        require(run.client.pane(agent.pane)["pid"] == pids[key],
                f"the engine restart replaced {key}'s process")
        run.until(lambda prompt=prompts[key]: ended(run.services, prompt),
                  f"{key}'s turn did not finish after the engine restart", seconds=240)
        ask(run, agent, "Reply with the single word pong.", "pong")


def handovers(run, agent):
    return [item for item in transcript(run.client, agent.pane) if item["kind"] == "handover"]


def switch(run, agent, target, model=None):
    """Switch the pane to harness `target`; it answers, remembers, and shows the handover."""
    before = len(handovers(run, agent))
    # The call returns when the new session is ready, which can take longer than a request.
    run.client.stream.settimeout(SWITCH_SECONDS)
    try:
        result = run.client.mutate("pane.harness.switch", paneId=agent.pane,
                                   harness=harness_id(target), **({"model": model} if model else {}))
    finally:
        run.client.stream.settimeout(10)
    require(result["toHarness"] == harness_id(target), "the switch went elsewhere")
    require(run.client.pane(agent.pane)["agent"]["harness"] == harness_id(target),
            "the pane does not show the new harness")
    agent.key = target
    run.until(lambda: len(handovers(run, agent)) > before, "the transcript shows no switch")
    reply = ask(run, agent, "What word did I ask you to remember? Reply with that word only.",
                REMEMBERED)
    run.note(f"switched to {target}; it answered {reply[:60]!r} and the transcript shows the switch")
    return current_model(run.client, agent.pane)


class ProviderError(Exception):
    """The harness's provider ended the turn with an error (an expired plan, no balance, no
    access to the model): the account behind the harness cannot be exercised."""

    def __init__(self, harness, notice):
        super().__init__(notice)
        self.harness = harness


def provider_error(run, agent, prompt, items):
    """The engine's system notice of a turn that ended with the `error` stop reason."""
    ended_in_error = any(event["stopReason"] == "error" for event in
                         events(run.services, "pane.turn_ended", agent.pane)
                         if event["promptId"] == prompt)
    notices = [item["text"] for item in items if item["role"] == "system"]
    return (notices[0] if notices else "the turn ended with an error") if ended_in_error else None


class Outcomes:
    """Per-harness results: a harness is passed, failed (with the step) or skipped (with why)."""

    def __init__(self, keys):
        self.harnesses = {key: {"status": "pending", "steps": {}} for key in keys}

    def skip(self, key, reason):
        self.harnesses[key].update(status="skipped", reason=reason)

    def fail(self, key, step, error):
        self.harnesses[key].update(status="failed", error=f"{step}: {error}")

    def alive(self, key):
        return self.harnesses[key]["status"] == "pending"

    def finish(self):
        """Close the run: a harness is passed only if a step ran and none failed."""
        for entry in self.harnesses.values():
            if entry["status"] == "pending":
                if entry["steps"]:
                    entry["status"] = "passed"
                else:
                    entry.update(status="skipped", reason="no step ran")
        return dict(self.harnesses)


def fail_step(run, outcomes, opened, key, name, error):
    outcomes.fail(key, name, describe_failure(error))
    run.note(f"{key}: FAILED {name}: {describe_failure(error)}")
    print_transcripts(run, key)
    drop_ended(outcomes, opened)
    return False


def drop_ended(outcomes, opened):
    for gone in [other for other in opened if not outcomes.alive(other)]:
        del opened[gone]


def guarded(run, outcomes, opened, key, name, action):
    """Run one step of one harness. A failure ends that harness only; the others carry on."""
    try:
        result = action()
    except SignedOut as error:
        # A sign-in that expires mid-run skips the harness without failing the run.
        outcomes.skip(error.harness, "its sign-in ended during the run")
        run.note(f"{error.harness}: skipped, its sign-in ended during the run")
        drop_ended(outcomes, opened)
        return False
    except ProviderError as error:
        if outcomes.harnesses[error.harness]["steps"]:
            # It answered before, so this is a failure of the run, not an unusable account.
            return fail_step(run, outcomes, opened, error.harness, name, error)
        # The provider behind the harness refused the account: nothing here can be exercised.
        outcomes.skip(error.harness, f"its provider ended the turn with an error: {error}"[:400])
        run.note(f"{error.harness}: skipped, provider error: {error}"[:400])
        drop_ended(outcomes, opened)
        return False
    except Exception as error:
        return fail_step(run, outcomes, opened, key, name, error)
    outcomes.harnesses[key]["steps"][name] = result or {"status": "passed"}
    return True


def choose_model(run, agent, model):
    """Start on a model the account accepts, as `--model HARNESS=MODEL` asked."""
    option = role_option(run.client, agent.pane, "model")
    values = [item["value"] for item in option["options"]] if option else []
    require(model in values, f"{agent.key} does not offer model {model!r}; it offers {values}")
    changed = run.client.mutate("pane.config.set", paneId=agent.pane, configId=option["id"],
                                value=model)
    require(changed["sessionPreserved"], f"{agent.key} replaced its session for the model")


def open_available(run, workspace, keys, models, outcomes, confirmed):
    opened = {}
    for key in keys:
        if not outcomes.alive(key):
            continue
        try:
            agent = run.open(workspace, key, confirm_sign_in=key in confirmed)
            if key in models:
                choose_model(run, agent, models[key])
        except SignedOut as error:
            reason = ("its sign-in was not confirmed" if error.harness in confirmed
                      else "not signed in (it asked for sign-in; this script never signs in)")
            outcomes.skip(error.harness, reason)
            run.note(f"{error.harness}: skipped, {reason}")
            continue
        except Exception as error:
            outcomes.fail(key, "open", describe_failure(error))
            run.note(f"{key}: FAILED open: {describe_failure(error)}")
            continue
        opened[key] = agent
        outcomes.harnesses[key]["model"] = current_model(run.client, agent.pane)
    return opened


def per_harness(run, outcomes, opened):
    for name, action in (("replies", reply_streams), ("selectors", selectors),
                         ("stop", stop_turn)):
        for key in list(opened):
            if key in opened:
                guarded(run, outcomes, opened, key, name,
                        lambda agent=opened[key]: action(run, agent))


def cross_step(run, report, name, action):
    """A step across harnesses: its failure is recorded and the later steps still run."""
    try:
        report[name] = action() or {"status": "passed"}
    except Exception as error:
        report[name] = {"status": "failed", "error": describe_failure(error)}
        run.note(f"FAILED {name}: {describe_failure(error)}")
        print_transcripts(run)


def messaging_ring(run, opened, report):
    """Each harness messages the next in a ring, so every one sends once and receives once."""
    keys = list(opened)
    if len(keys) < 2:
        report["messaging"] = {"status": "skipped", "reason": "needs two available harnesses"}
        return
    for sender, recipient in zip(keys, keys[1:] + keys[:1], strict=True):
        cross_step(run, report, f"{sender}_messages_{recipient}",
                   lambda: exchange(run, opened[sender], opened[recipient]))


def switch_through(run, opened, workspace, models, confirmed, report):
    # The engine's switch candidate has no protocol sign-in step, so a harness that needs one
    # (Cursor) is not a switch target.
    keys = [key for key in opened if key not in confirmed]
    if len(keys) < 2:
        report["switch_through_all"] = {
            "status": "skipped",
            "reason": "needs two available harnesses that sign in without a protocol step"}
        return
    first = keys[0]

    def through():
        # A pane of its own for the switch, so the messaging panes keep their harness.
        switcher = run.open(workspace, first)
        if first in models:
            choose_model(run, switcher, models[first])
        ask(run, switcher, f"Remember the word {REMEMBERED}. Reply with the single word pong.",
            "pong")
        return {"status": "passed",
                "models": {key: switch(run, switcher, key, models.get(key))
                           for key in [*keys[1:], first]}}

    cross_step(run, report, "switch_through_all", through)


def restart(run, opened, report):
    def during_turns():
        restart_during_turns(run, opened)
        return {"status": "passed", "harnesses": sorted(opened)}

    if opened:
        cross_step(run, report, "engine_restart_during_turns", during_turns)


def scenarios(run, workspace, keys, models, confirmed, outcomes, report):
    opened = open_available(run, workspace, keys, models, outcomes, confirmed)
    per_harness(run, outcomes, opened)
    messaging_ring(run, opened, report)
    switch_through(run, opened, workspace, models, confirmed, report)
    restart(run, opened, report)


def unavailable(client, keys):
    """The reason, per harness, that this machine cannot run it; absent when it can."""
    reasons = {}
    packages = [key for key in keys if key in HARNESSES]
    if packages and not (shutil.which("node") and shutil.which("npm")):
        for key in packages:
            reasons[key] = "Node.js 22 or later with npm is required to install its adapter"
    listed, cursor = {}, None
    while True:
        page = client.call("harness.list", {"limit": 64, **({"cursor": cursor} if cursor else {})})
        listed.update({item["id"]: item for item in page["items"]})
        cursor = page.get("nextCursor")
        if not cursor:
            break
    for key in keys:
        if key not in HARNESSES and not listed.get(harness_id(key), {}).get("available"):
            reasons[key] = "its command was not found on the engine's launch PATH"
    return reasons


def run_real(binaries, keys, models, confirmed, report):
    outcomes = Outcomes(keys)
    report["harnesses"] = outcomes.harnesses
    with tempfile.TemporaryDirectory(prefix="soyeht-real-", ignore_cleanup_errors=True) as directory:
        services = Services(binaries)
        run = None
        try:
            packaged = [key for key in keys if key in HARNESSES and shutil.which("npm")
                        and shutil.which("node")]
            adapters = register_adapters(services, packaged, Path(directory))
            # The engine asks the person's login shell for the PATH that finds `cursor-agent` and
            # `opencode`; the suites' prompt-only ZDOTDIR would hide the person's own.
            services.set_environment(
                "engine", ZDOTDIR=os.environ.get("ZDOTDIR") or str(Path.home()))
            services.start("ptyd")
            services.start("engine")
            client = services.client("real-harnesses")
            for key, reason in unavailable(client, keys).items():
                outcomes.skip(key, reason)
            project = services.data / "project"
            project.mkdir()
            quiet_workspace(project, adapters)
            workspace = client.mutate("workspace.create", name="Real harnesses",
                                      path=str(project))["workspace"]["workspaceId"]
            run = Run(services, client)
            scenarios(run, workspace, keys, models, confirmed, outcomes, report["steps"])
        except Exception:
            print_transcripts(run)
            raise
        finally:
            if run:
                report["log"] = run.log
            report["harnesses"] = outcomes.finish()
            services.close()


def build_binaries():
    """The programs the acceptance starts, from this checkout (incremental after the first run)."""
    packages = ("soyeht-engine", "soyeht-ptyd", "soyeht-cli", "soyeht-mcp")
    subprocess.run(["cargo", "build", "-j", os.environ.get("SOYEHT_JOBS", "4"),
                    *(arg for package in packages for arg in ("-p", package))],
                   cwd=ROOT / "engine", check=True)


def verdict(report):
    """A run fails when a harness failed, or when no harness ran at all."""
    statuses = {key: entry["status"] for key, entry in report["harnesses"].items()}
    report["skipped"] = {key: report["harnesses"][key]["reason"]
                         for key, status in statuses.items() if status == "skipped"}
    failed = sorted(key for key, status in statuses.items() if status == "failed")
    failed += sorted(name for name, step in report["steps"].items()
                     if step.get("status") == "failed")
    if failed and "error" not in report:
        report["error"] = "failed: " + ", ".join(failed)
    if "passed" not in statuses.values() and "error" not in report:
        report["error"] = "no harness ran: " + json.dumps(report["skipped"])


def main():
    parser = argument_parser(
        __doc__, Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "engine/target")) / "debug")
    parser.add_argument("--build", action="store_true",
                        help="build the engine programs from this checkout first (needs cargo)")
    parser.add_argument("--harness", choices=ALL_HARNESSES, action="append",
                        help="run only this harness (repeat for several); default: all four")
    parser.add_argument("--model", action="append", default=[], metavar="HARNESS=MODEL",
                        help="select a model option for a harness, such as codex=gpt-5.5")
    parser.add_argument("--confirm-sign-in", action="append", default=[], choices=ALL_HARNESSES,
                        metavar="HARNESS",
                        help="press the pane's protocol sign-in for this harness, as the app's "
                             "button does (Cursor needs it for every process; it reuses the CLI's "
                             "existing login). Without it the harness is skipped as not signed in.")
    args = parser.parse_args()
    authorize_real_run()
    if args.build:
        build_binaries()
    binaries = args.bin_dir.resolve()
    require_binaries(binaries, PRODUCT_NAMES,
                     "build workspace binaries before running the real harness acceptance")
    started = time.monotonic()
    keys = [key for key in ALL_HARNESSES if key in (args.harness or ALL_HARNESSES)]
    report = {"harnesses": {}, "steps": {}}
    try:
        run_real(binaries, keys, dict(choice.split("=", 1) for choice in args.model),
                 set(args.confirm_sign_in), report)
    except Exception as error:
        report["error"] = describe_failure(error)
    verdict(report)
    finish_timed_report(report, started, args.report)


if __name__ == "__main__":
    main()
