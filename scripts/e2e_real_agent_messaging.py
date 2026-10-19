#!/usr/bin/env python3
"""Prove agent-to-agent messaging between real harness panes.

The default pair is a Claude Code pane that messages a Codex pane. The run uses
a disposable dev profile and the pinned ACP registry adapters. A harness that
is not signed in is reported by name and fails the run; `--pair codex codex`
still proves real delivery. Fake-agent engine tests cover the same ordering
deterministically. `--mode codex=read-only` runs a harness in its approval
mode: every Soyeht tool call must then be presented as
"Use soyeht-workspace · <tool>", approved, and the reply still delivered.
"""

import json
import tempfile
import time
from pathlib import Path

from e2e import (
    PRODUCT_NAMES,
    Services,
    argument_parser,
    describe_failure,
    finish_timed_report,
    require,
    require_binaries,
)
from e2e_agents import events, question, transcript
from e2e_real_acp import (
    HARNESSES,
    authorize_real_run,
    install_adapter,
    pinned_package,
    quiet_workspace,
    set_mode,
)


SERVER = "soyeht-workspace"
SOYEHT_TOOLS = ("list_agents", "message_agent")
TURN_SECONDS = 300
HELD_SECONDS = 25


class SignedOut(Exception):
    def __init__(self, harness):
        super().__init__(f"{harness} is not signed in")
        self.harness = harness


class Agent:
    def __init__(self, key, pane, name):
        self.key = key
        self.pane = pane
        self.name = name
        self.mode = None
        self.approvals = 0


class Run:
    def __init__(self, services, client):
        self.services = services
        self.client = client
        self.agents = []
        self.draft = None
        self.draft_refreshed = 0.0
        self.log = []

    def note(self, text):
        self.log.append(text)
        print(text, flush=True)

    def refresh_draft(self):
        # Clients renew a draft while the composer holds text.
        if self.draft and time.monotonic() - self.draft_refreshed > 5:
            self.client.mutate("pane.draft", paneId=self.draft, active=True)
            self.draft_refreshed = time.monotonic()

    def approve_soyeht_tools(self, agent):
        # A request the engine cannot present is refused with a transcript notice.
        blocked = [update for update in events(self.services, "pane.update", agent.pane)
                   if "blocked this permission request" in json.dumps(update)]
        require(not blocked, f"{agent.key} permission request was blocked: {blocked[:1]}")
        pending = question(self.client, agent.pane)
        if pending is None:
            return
        require(pending.get("title") in {f"Use {SERVER} · {tool}" for tool in SOYEHT_TOOLS},
                f"unexpected permission request: {pending.get('title')!r}")
        allow = next(option["optionId"] for option in pending["options"]
                     if option["kind"] == "allow_once")
        self.client.mutate("pane.permission.answer", paneId=agent.pane,
                           requestId=pending["requestId"], optionId=allow)
        agent.approvals += 1
        self.note(f"{agent.name}: approved {pending['title']!r}")

    def until(self, action, message, seconds=TURN_SECONDS):
        deadline = time.monotonic() + seconds
        while True:
            for agent in self.agents:
                # Adapters report an expired sign-in at session open or mid-turn.
                if events(self.services, "pane.needs_login", agent.pane):
                    raise SignedOut(agent.key)
                self.approve_soyeht_tools(agent)
            self.require_own_server()
            self.refresh_draft()
            value = action()
            if value:
                return value
            require(time.monotonic() < deadline, message)
            time.sleep(0.5)

    def open(self, workspace, key, model=None, mode=None):
        pane = self.client.mutate("pane.create", workspaceId=workspace, mode="agent",
                                  harness=f"real-{key}")["paneId"]
        agent = Agent(key, pane, None)
        self.agents.append(agent)
        def session_or_sign_in():
            session = self.client.pane(pane).get("agent", {})
            return session if session.get("sessionId") or session.get("authMethods") else None

        if not self.until(session_or_sign_in, f"{key} session did not open",
                          seconds=240).get("sessionId"):
            # The adapter offers sign-in methods instead of a session.
            self.agents.remove(agent)
            raise SignedOut(key)
        if model:
            # A host's default model may not be available through the pinned adapter.
            changed = self.client.mutate("pane.config.set", paneId=pane,
                                         configId="model", value=model)
            require(changed["sessionPreserved"], f"{key} model change replaced the session")
        if mode:
            set_mode(self.client, pane, mode)
            self.agents[-1].mode = mode
        self.agents[-1].name = self.client.pane(pane)["name"]
        self.note(f"{key}: session ready as {self.agents[-1].name!r}")
        return self.agents[-1]

    def queue(self, agent):
        return self.client.call("pane.queue.list", {"paneId": agent.pane, "limit": 64})["items"]

    def settled(self):
        return all(self.client.pane(agent.pane)["state"] == "idle" and not self.queue(agent)
                   for agent in self.agents)

    def require_own_server(self):
        # Hosts may configure other Soyeht servers with the same tool names; a
        # pane must reach its peers only through the server bound to it.
        for update in events(self.services, "pane.update"):
            title = (update.get("tool") or {}).get("title", "").lower()
            require("soyeht" not in title or SERVER in title.replace("_", "-"),
                    f"an agent called another Soyeht server: {title}")

    def user_messages(self, agent):
        return [item for item in transcript(self.client, agent.pane)
                if item["role"] == "user" and item["kind"] == "message"]

    def peer_message(self, recipient, sender, phrase):
        # A peer bubble carries its sender by name and never the routing identifier.
        for item in self.user_messages(recipient):
            source = item.get("agentMessage", {})
            if source.get("paneId") == sender.pane and phrase in item["text"].lower():
                require(source["name"] == sender.name and sender.pane not in item["text"],
                        "peer message is not attributed by display name")
                return item
        return None


def exchange(run, sender, recipient):
    ping, pong = f"ping from {sender.key}", f"pong from {recipient.key}"
    run.client.mutate("pane.prompt", paneId=sender.pane, text=(
        f"Use the list_agents tool of the {SERVER} MCP server to find the agent "
        f"named {recipient.name!r}. "
        f"Then use its message_agent tool to send it exactly this text: "
        f"\"{ping}. Reply to me with message_agent and exactly: {pong}\". "
        f"Do not do anything else."))
    run.until(lambda: run.peer_message(recipient, sender, ping),
              f"{recipient.name} did not receive the attributed ping")
    run.note(f"{recipient.name} received {ping!r} attributed to {sender.name}")
    run.until(lambda: run.peer_message(sender, recipient, pong),
              f"{sender.name} did not receive the attributed pong")
    run.note(f"{sender.name} received {pong!r} attributed to {recipient.name}")
    run.until(run.settled, "the exchange did not settle")
    for agent in (sender, recipient):
        # In an approval mode, the reply above proves the approved call ran.
        require(agent.mode is None or agent.approvals > 0,
                f"{agent.key} in mode {agent.mode} never asked to use a Soyeht tool")


def draft_hold(run, sender, recipient):
    before = len(run.user_messages(recipient))
    note = f"second ping from {sender.key}"
    run.draft = recipient.pane
    run.refresh_draft()
    run.client.mutate("pane.prompt", paneId=sender.pane, text=(
        f"Use the message_agent tool of the {SERVER} MCP server to send the agent "
        f"named {recipient.name!r} "
        f"exactly this text: \"{note}\". Do not wait for a reply."))
    run.until(lambda: any(item.get("agentMessage", {}).get("paneId") == sender.pane
                          for item in run.queue(recipient)),
              "the second ping did not wait in the recipient's queue")
    held_until = time.monotonic() + HELD_SECONDS
    run.until(lambda: time.monotonic() > held_until, "hold window failed",
              seconds=HELD_SECONDS + 5)
    require(len(run.user_messages(recipient)) == before,
            "an agent message was delivered while the user had a draft")
    run.note(f"{note!r} held for {HELD_SECONDS}s while the user's draft existed")
    draft = "Draft typed by the user: what is 2 + 2? Answer with the number only."
    # The composer sends its text; the engine ends the draft with that prompt.
    run.draft = None
    run.client.mutate("pane.prompt", paneId=recipient.pane, text=draft)
    run.until(lambda: len(run.user_messages(recipient)) >= before + 2,
              "the held message was not delivered after the draft was sent")
    typed, held = run.user_messages(recipient)[before:before + 2]
    require(typed["text"] == draft and "agentMessage" not in typed,
            "the user's draft was not delivered first and intact")
    require(held.get("agentMessage", {}).get("name") == sender.name
            and held["text"].lower().strip(" .\"") == note,
            "the agent message was not delivered separately after the draft")
    run.note("user draft delivered intact, then the attributed agent message on its own")
    run.until(run.settled, "panes did not settle after the draft")


def register_adapters(services, pair, directory):
    # The pinned registry adapters, launched without the user's own extensions
    # or login-shell PATH, so nothing they start reaches protected folders.
    entries = []
    for key in sorted(set(pair)):
        package, version = pinned_package(HARNESSES[key])
        adapter = install_adapter(directory / key, package, version)
        entries.append({"id": f"real-{key}", "name": key.capitalize(),
                        "command": str(adapter), "args": [], "version": version})
    (services.data / "harnesses.json").write_text(json.dumps(entries))
    return [Path(entry["command"]) for entry in entries]


def print_transcripts(run):
    """The transcript of a failing run; no credentials reach it."""
    for agent in run.agents if run else []:
        for item in transcript(run.client, agent.pane):
            print(f"{agent.name} {item['role']}/{item['kind']}: {json.dumps(item)[:600]}")


def run_real(binaries, pair, models, modes):
    with tempfile.TemporaryDirectory(prefix="soyeht-real-messaging-") as directory:
        services = Services(binaries)
        run = None
        try:
            adapters = register_adapters(services, pair, Path(directory))
            services.start("ptyd")
            services.start("engine")
            client = services.client("real-messaging")
            project = services.data / "project"
            project.mkdir()
            quiet_workspace(project, adapters)
            workspace = client.mutate("workspace.create", name="Real agent messaging",
                                      path=str(project))["workspace"]["workspaceId"]
            run = Run(services, client)
            sender, recipient = (run.open(workspace, key, models.get(key), modes.get(key))
                                 for key in pair)
            exchange(run, sender, recipient)
            draft_hold(run, sender, recipient)
            return run.log
        except Exception:
            print_transcripts(run)
            raise
        finally:
            services.close()


def main():
    parser = argument_parser(__doc__)
    parser.add_argument("--pair", nargs=2, choices=sorted(HARNESSES),
                        default=["claude", "codex"], metavar=("SENDER", "RECIPIENT"))
    parser.add_argument("--model", action="append", default=[], metavar="HARNESS=MODEL",
                        help="select a model option for a harness, such as codex=gpt-5.5")
    parser.add_argument("--mode", action="append", default=[], metavar="HARNESS=MODE",
                        help="run a harness in a permission mode, such as codex=read-only")
    args = parser.parse_args()
    authorize_real_run()
    binaries = args.bin_dir.resolve()
    require_binaries(binaries, PRODUCT_NAMES,
                     "build workspace binaries before running the real messaging acceptance")
    started = time.monotonic()
    report = {"pair": args.pair}
    try:
        models = dict(choice.split("=", 1) for choice in args.model)
        modes = dict(choice.split("=", 1) for choice in args.mode)
        report["modes"] = modes
        report["log"] = run_real(binaries, args.pair, models, modes)
    except Exception as error:
        report["error"] = describe_failure(error)
    finish_timed_report(report, started, args.report)


if __name__ == "__main__":
    main()
