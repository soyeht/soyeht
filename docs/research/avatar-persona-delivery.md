# Avatar persona delivery (slice 2 of ADR 0166)

How the avatar's name and persona reach each harness, by the channel the harness really honours.
Run on 2026-11-15 on macOS (arm64), headless: each case starts the harness's ACP adapter over
stdio (the way the engine does), opens one session in a scratch folder, asks one question and reads
the reply. No terminal interface of any harness was started, and no test rig typed into one.

## Versions

| | Adapter or program | Version | Model |
|---|---|---|---|
| Claude Code | `@agentclientprotocol/claude-agent-acp` (what the engine launches) | 0.100.0 | the adapter's default |
| Codex | `@agentclientprotocol/codex-acp` over the installed `codex` | 1.11.0 over 0.169.0 | the default model was refused by the account in use, so a smaller one was chosen with `session/set_model` |
| OpenCode | `opencode acp` | 2.2.5 | its default (a free model) |
| Cursor Agent | `cursor-agent acp` | 2026.09.02 | not reached, see below |

## Method

- **The persona** is `Your name is Marlow, the chief agent of this project. Begin every reply with the
  exact word Marlow: followed by one short sentence. Never use Markdown.` The question is `Who are
  you? Answer in one sentence.` A case is **honoured** when the reply to the question starts with
  `Marlow`. A `control` case with no persona never does.
- **Channels** (one case each, a fresh scratch folder and a fresh process per case):
  `system`, the ACP `systemPrompt` of `session/new` (`mode: append`); `file-claude` and `file-agents`,
  the persona as `CLAUDE.md` or `AGENTS.md` in the session's folder; `first`, the persona sent as the
  first prompt and the question as the second; `first-quiet`, as `first` with the sentence Soyeht
  adds ("These are your standing instructions ... do not answer them"); `codexcfg`, the persona as
  `developer_instructions` in the `CODEX_CONFIG` environment variable of the Codex adapter.
- The adapters ran on the owner's existing logins for one or two tiny turns per case. Nothing was
  logged in or out, no user-level configuration was written (the harnesses write their own session
  files), and a request the agent made of the client (permissions, files, terminals) was refused or
  cancelled: the client advertised no capability.
- The probe is `persona_probe.py`, quoted at the end. It is not committed (there is no `scripts/research`).

## What each adapter advertises

`initialize` answers `agentCapabilities.sessionCapabilities`:

| | Advertised |
|---|---|
| Claude Code | `additionalDirectories`, `close`, `delete`, `fork`, `list`, `resume`, **`systemPrompt`** (`mode: {override: {}}`) |
| Codex | `resume`, `list`, `close`, `delete`, `fork`, `additionalDirectories`, `subagents`; **no `systemPrompt`** |
| OpenCode | `additionalDirectories`, `close`, `delete`, `fork`, `list`, `resume`; **no `systemPrompt`** |
| Cursor Agent | `list` only; **no `systemPrompt`** |

## Results

| Channel | Claude Code | Codex | OpenCode | Cursor Agent |
|---|---|---|---|---|
| control (no persona) | no | no | no | not measured |
| ACP `systemPrompt` | **yes** | **no: accepted and ignored, no error** | **no: accepted and ignored, no error** | not measured |
| `CLAUDE.md` in the folder | yes | no | no | not measured |
| `AGENTS.md` in the folder | yes | yes | yes | not measured |
| first prompt | yes | yes | yes | not measured |
| first prompt with Soyeht's sentence | yes | yes | yes | not measured |
| `CODEX_CONFIG` `developer_instructions` | n/a | yes | n/a | n/a |

Reading the table:

- **The `systemPrompt` of a session is honoured exactly where it is advertised, and ignored without an
  error where it is not.** An adapter that does not advertise it cannot be told apart from one that
  applies it by the answer to `session/new`, so the field is sent only to an adapter that advertises it.
- **After `session/load`** (a second process, the same session) the Claude adapter still answered as
  Marlow with the prompt sent again and without it. The second is no proof that the session keeps the
  system prompt: the earlier replies of the session all begin with `Marlow:` and the model can follow
  them. Soyeht sends the persona again on every `session/new` and `session/load`.
- **The files**: `AGENTS.md` is read by all three harnesses that were reached, the Claude adapter
  included; `CLAUDE.md` by Claude Code only.
- **The first prompt is always honoured, and costs one turn.** The sentence "do not answer them" does
  not stop the harness from answering: all three replied in one short line ("Understood.") and went
  on to answer the question as Marlow. Without the sentence Claude Code answered at length. The reply
  to that prompt is in the pane's transcript, and a client that speaks replies can skip a reply to an
  automatic prompt.
- **Codex takes a per-launch configuration** through the adapter's environment (`CODEX_CONFIG`), which
  is a way to hand it instructions with no turn and no file.

## What was not measured

- **Cursor Agent.** `initialize` answers, but `session/new` answers `Authentication required`, and its
  only authentication method (`cursor_login`) is a login in a browser, which the spike does not perform.
  What it advertises says it has no `systemPrompt`, so Soyeht gives it the first prompt, which every
  harness measured honours.
- **Compaction.** A persona given as the first prompt is part of the conversation, and a harness that
  summarises a long conversation may drop or shorten it. The system prompt is not part of the
  conversation. Not measured.
- **Windows and Linux**, and **a persona of the size limit** (`avatarPersonaChars`).
- **Which instruction wins** when the project's own `AGENTS.md` and the persona disagree.

## What Soyeht does

The channel follows the table, and nothing in it is a rule about behaviour (`agents::persona`):

1. An adapter that advertises `systemPrompt` is sent the persona as the session's system prompt,
   appended, in `session/new` and in `session/load`.
2. Any other harness gets the persona as the first prompt of a new session, queued as an automatic
   prompt, with the sentence that says it is not a request.
3. A file in the project folder is not used. Every harness that has no system prompt reads
   `AGENTS.md`, but a workspace can be a folder the person already has, with a file of their own, and
   Soyeht writes in the person's folder nothing the person did not ask for.
4. The persona is read from the workspace's binding each time a session opens, so the text the person
   last saved is the one the next session gets.

The text is the avatar's name (`You are <name>, the avatar agent of this project.`) followed by the
person's own persona text. The sentences about how the avatar speaks are text the configuration screen
offers the person, not something the engine adds.

## The probe

```python
"""Persona delivery probe: drives one ACP agent headless over stdio, one case per run.

usage: persona_probe.py HARNESS CASE [--restart]
HARNESS: claude | codex | opencode | cursor
CASE: control | system | file-agents | file-claude | first | codexcfg
Prints one JSON line: what the agent advertised and what its replies were.
"""

import json
import os
import queue
import subprocess
import sys
import tempfile
import threading
import time

PERSONA = (
    "Your name is Marlow, the chief agent of this project. Begin every reply with the exact "
    "word Marlow: followed by one short sentence. Never use Markdown."
)
QUESTION = "Who are you? Answer in one sentence."
ADAPTERS = {
    "claude": (["npx", "--yes", "@agentclientprotocol/claude-agent-acp@0.100.0"], {}),
    "codex": (["npx", "--yes", "@agentclientprotocol/codex-acp@1.11.0"], {}),
    "opencode": (["opencode", "acp"], {}),
    "cursor": (["cursor-agent", "acp"], {}),
}
TIMEOUT = 150


class Agent:
    def __init__(self, command, cwd, env):
        self.process = subprocess.Popen(
            command,
            cwd=cwd,
            env={**os.environ, **env},
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
        )
        self.lines = queue.Queue()
        self.next_id = 1
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.process.stdout:
            try:
                self.lines.put(json.loads(line))
            except ValueError:
                pass
        self.lines.put(None)

    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def request(self, method, params):
        ident = self.next_id
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": ident, "method": method, "params": params})
        text = []
        deadline = time.time() + TIMEOUT
        while time.time() < deadline:
            try:
                message = self.lines.get(timeout=1)
            except queue.Empty:
                continue
            if message is None:
                return {"error": {"message": "agent exited"}}, "".join(text)
            if "method" in message and "id" in message:
                # An agent-to-client request: refuse tools, cancel permissions.
                if message["method"] == "session/request_permission":
                    reply = {"outcome": {"outcome": "cancelled"}}
                    self.send({"jsonrpc": "2.0", "id": message["id"], "result": reply})
                else:
                    error = {"code": -32601, "message": "not supported"}
                    self.send({"jsonrpc": "2.0", "id": message["id"], "error": error})
            elif message.get("method") == "session/update":
                update = message["params"]["update"]
                if update.get("sessionUpdate") == "agent_message_chunk":
                    content = update.get("content", {})
                    if content.get("type") == "text":
                        text.append(content["text"])
            elif message.get("id") == ident:
                return message, "".join(text)
        return {"error": {"message": "timeout"}}, "".join(text)

    def close(self):
        try:
            self.process.stdin.close()
            self.process.wait(timeout=5)
        except Exception:
            self.process.kill()


def main():
    harness, case = sys.argv[1], sys.argv[2]
    command, env = ADAPTERS[harness]
    command = list(command)
    env = dict(env)
    cwd = tempfile.mkdtemp(prefix=f"persona-{harness}-{case}-", dir=os.environ["PROBE_DIR"])
    if case == "file-agents":
        open(os.path.join(cwd, "AGENTS.md"), "w").write(PERSONA + "\n")
    if case == "file-claude":
        open(os.path.join(cwd, "CLAUDE.md"), "w").write(PERSONA + "\n")
    if case == "codexcfg":
        env["CODEX_CONFIG"] = json.dumps({"developer_instructions": PERSONA})
    agent = Agent(command, cwd, env)
    out = {"harness": harness, "case": case}
    reply, _ = agent.request(
        "initialize", {"protocolVersion": 1, "clientCapabilities": {}}
    )
    result = reply.get("result", {})
    capabilities = result.get("agentCapabilities", {})
    out["version"] = result.get("agentInfo")
    out["sessionCapabilities"] = capabilities.get("sessionCapabilities")
    out["loadSession"] = capabilities.get("loadSession")
    params = {"cwd": cwd, "mcpServers": []}
    if case == "system":
        params["systemPrompt"] = {
            "mode": "append",
            "content": [{"type": "text", "text": PERSONA}],
        }
    reply, _ = agent.request("session/new", params)
    if "error" in reply and result.get("authMethods"):
        method = result["authMethods"][0]["id"]
        auth, _ = agent.request("authenticate", {"methodId": method})
        out["authenticate"] = "error" in auth
        reply, _ = agent.request("session/new", params)
    if "error" in reply:
        out["new_error"] = reply["error"]
        print(json.dumps(out))
        agent.close()
        return
    session = reply["result"]["sessionId"]
    out["new_keys"] = sorted(reply["result"].keys())
    models = reply["result"].get("models") or {}
    out["models"] = [m.get("modelId") or m.get("id") for m in models.get("availableModels", [])][:12]
    out["current_model"] = models.get("currentModelId")
    if os.environ.get("MODEL"):
        chosen, _ = agent.request(
            "session/set_model", {"sessionId": session, "modelId": os.environ["MODEL"]}
        )
        out["set_model_error"] = chosen.get("error")
    prompts = [QUESTION]
    if case == "first":
        prompts = [PERSONA, QUESTION]
    if case == "first-quiet":
        note = (
            "These are your standing instructions for this conversation, not a request from "
            "the person: do not answer them."
        )
        prompts = [PERSONA + "\n\n" + note, QUESTION]
    replies = []
    for prompt in prompts:
        done, text = agent.request(
            "session/prompt",
            {"sessionId": session, "prompt": [{"type": "text", "text": prompt}]},
        )
        replies.append(text if "result" in done else f"ERROR {done.get('error')}")
    out["replies"] = replies
    out["honoured"] = replies[-1].strip().lstrip("*_` ").startswith("Marlow")
    if "--restart" in sys.argv:
        agent.close()
        again = Agent(command, cwd, env)
        again.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        model = os.environ.get("MODEL")
        load = {"sessionId": session, "cwd": cwd, "mcpServers": []}
        if case == "system" and os.environ.get("RESEND", "1") == "1":
            load["systemPrompt"] = params["systemPrompt"]
        loaded, _ = again.request("session/load", load)
        if "error" in loaded:
            out["load_error"] = loaded["error"]
        else:
            done, text = again.request(
                "session/prompt",
                {"sessionId": session, "prompt": [{"type": "text", "text": QUESTION}]},
            )
            out["after_load"] = text
            out["honoured_after_load"] = text.strip().lstrip("*_` ").startswith("Marlow")
        again.close()
    else:
        agent.close()
    print(json.dumps(out))


main()
```
