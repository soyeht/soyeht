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
  first prompt and the question as the second; `first-quiet`, as `first` with a sentence saying the
  message is not a request; `prepend`, the persona as a text block ahead of the question in the same
  prompt (the label and last line are the ones Soyeht uses); `codexcfg`, the persona as
  `developer_instructions` in the `CODEX_CONFIG` environment variable of the Codex adapter.
- The adapters ran on the owner's existing logins for one or two tiny turns per case. Nothing was
  logged in or out, no user-level configuration was written (the harnesses write their own session
  files), and a request the agent made of the client (permissions, files, terminals) was refused or
  cancelled: the client advertised no capability.
- **Runs.** Each cell of the first results table is **one run**: one session, one reply. A single
  run says that a channel can work, not how often. The `prepend` case, the one Soyeht uses, was run
  **three times per harness** with the final text. An earlier set of runs with the same block without its
  last line answered the question as the persona in every run too.
- The probe is `persona_probe.py`, quoted at the end. It is not committed (there is no `scripts/research`).

## What each adapter advertises

`initialize` answers `agentCapabilities.sessionCapabilities`:

| | Advertised |
|---|---|
| Claude Code | `additionalDirectories`, `close`, `delete`, `fork`, `list`, `resume`, **`systemPrompt`** (`mode: {override: {}}`) |
| Codex | `resume`, `list`, `close`, `delete`, `fork`, `additionalDirectories`, `subagents`; **no `systemPrompt`** |
| OpenCode | `additionalDirectories`, `close`, `delete`, `fork`, `list`, `resume`; **no `systemPrompt`** |
| Cursor Agent | `list` only; **no `systemPrompt`** |

`systemPrompt` is a field of the protocol's **unstable** surface: the Claude adapter takes the error
type of its validation from the SDK's `unstable` entry point, and a version of the protocol or of an
adapter may change or remove it. Soyeht sends it only to an adapter that advertises it on this
version, and reads the advertisement again each time the adapter starts.

## Results

| Channel | Claude Code | Codex | OpenCode | Cursor Agent |
|---|---|---|---|---|
| control (no persona) | no | no | no | not measured |
| ACP `systemPrompt` | **yes** | **no: accepted and ignored, no error** | **no: accepted and ignored, no error** | not measured |
| `CLAUDE.md` in the folder | yes | no | no | not measured |
| `AGENTS.md` in the folder | yes | yes | yes | not measured |
| first prompt | yes | yes | yes | not measured |
| first prompt with the sentence | yes | yes | yes | not measured |
| **block ahead of the question, 3 runs** | **3 of 3** | **3 of 3** | **3 of 3** | not measured |
| `CODEX_CONFIG` `developer_instructions` | n/a | yes | n/a | n/a |

Reading the table (every cell but the block row is one run):

- **The `systemPrompt` of a session is honoured exactly where it is advertised, and ignored without an
  error where it is not.** An adapter that does not advertise it cannot be told apart from one that
  applies it by the answer to `session/new`, so the field is sent only to an adapter that advertises it.
- **After `session/load`** (a second process, the same session) the Claude adapter still answered as
  Marlow with the prompt sent again and without it. The second is no proof that the session keeps the
  system prompt: the earlier replies of the session all begin with `Marlow:` and the model can follow
  them. Soyeht sends the persona again on every `session/new` and `session/load`.
- **A persona sent as a prompt of its own is honoured in the run measured, and costs one turn.** The
  sentence saying it is not a request does not stop the harness from answering: all three replied in
  one short line ("Understood.") and went on to answer the question as Marlow. Without the sentence
  Claude Code answered at length.
- **A persona sent as a text block ahead of the question costs no turn and was honoured in every run
  measured** (3 of 3 on each of the three harnesses, with the harness answering the question and nothing
  else). After a restart with `session/load` the next answer was still the persona's in 8 of the 8
  runs that got to it; the ninth (OpenCode) did not load, and the Codex runs of the first set of three
  did not count because the rig had not set the small model again after the restart.
- **What a replay shows.** `session/load` replays the user's message. The Claude and Codex adapters
  replay the block and the question as two messages; OpenCode replays them as one message with a line
  break between. Soyeht cuts the block out of a replayed message by its first and last lines.
- **The files**: `AGENTS.md` is read by the three harnesses that were reached, the Claude adapter
  included; `CLAUDE.md` by Claude Code only. Cursor Agent was not reached, so nothing is claimed for it.
- **Codex takes a per-launch configuration** through the adapter's environment (`CODEX_CONFIG`), which
  is a way to hand it instructions with no turn and no file.

## What was not measured

- **Cursor Agent.** `initialize` answers, but `session/new` answers `Authentication required`, and its
  only authentication method (`cursor_login`) is a login in a browser, which the spike does not perform.
  What it advertises says it has no `systemPrompt`, so Soyeht gives it the block ahead of the first
  prompt, which every harness measured honours. Whether it does is not measured.
- **Compaction.** A persona given ahead of a prompt is part of the conversation, and a harness that
  summarises a long conversation may drop or shorten it. The system prompt is not part of the
  conversation. Not measured.
- **Windows and Linux**, and **a persona of the size limit** (`avatarPersonaChars`).
- **Which instruction wins** when the project's own `AGENTS.md` and the persona disagree.

## What Soyeht does

The channel follows the table, and nothing in it is a rule about behaviour (`agents::persona`):

1. An adapter that advertises `systemPrompt` is sent the persona as the session's system prompt,
   appended, in `session/new` and in `session/load`.
2. Any other harness is owed the persona by its next prompt: when its session has been **created**
   (`session/new` answered, not tried) and when the person changed the persona of the open pane, the
   pane is marked, and the next `session/prompt` the engine builds carries the persona as a text block
   ahead of its own. It is no prompt of the queue, so it cannot be removed, paused, or overtaken by a
   prompt the person wrote, and it costs no turn. A `session/load` does not send it again: the
   conversation it loads has it.
3. A file in the project folder is not used. The harnesses measured read `AGENTS.md`, but a workspace
   can be a folder the person already has, with a file of their own, and Soyeht writes in the
   person's folder nothing the person did not ask for.
4. The persona is read from the workspace's binding each time it is delivered, so the text the person
   last saved is the one the next delivery carries.

The text is the avatar's name (`You are <name>, the avatar agent of this project.`) followed by the
person's own persona text. The sentences about how the avatar speaks are text the configuration screen
offers the person, not something the engine adds.

## The probe

```python
"""Persona delivery probe: drives one ACP agent headless over stdio, one case per run.

usage: PROBE_DIR=<scratch folder> [MODEL=<model id>] [RESEND=0] persona_probe.py HARNESS CASE [--restart]
HARNESS: claude | codex | opencode | cursor
CASE: control | system | file-agents | file-claude | first | first-quiet | prepend | codexcfg
  prepend: the persona as a text block ahead of the question, in one prompt
--restart: closes the process, opens the session again with session/load, asks once more, and
  records the user messages the load replays.
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
        self.user_chunks = []
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
                if update.get("sessionUpdate") == "user_message_chunk":
                    self.user_chunks.append(update.get("content", {}).get("text"))
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
    prepend = False
    if case == "prepend":
        prepend = True
    if case == "first-quiet":
        note = (
            "These are your standing instructions for this conversation, not a request from "
            "the person: do not answer them."
        )
        prompts = [PERSONA + "\n\n" + note, QUESTION]
    replies = []
    for prompt in prompts:
        blocks = [{"type": "text", "text": prompt}]
        if prepend:
            label = (
                "Standing instructions for this conversation (they come from the application, "
                "not from the person):\n\n"
            )
            blocks.insert(0, {"type": "text", "text": label + PERSONA + "\n\n(End of the standing instructions.)"})
        done, text = agent.request("session/prompt", {"sessionId": session, "prompt": blocks})
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
            if os.environ.get("MODEL"):
                again.request(
                    "session/set_model", {"sessionId": session, "modelId": os.environ["MODEL"]}
                )
            done, text = again.request(
                "session/prompt",
                {"sessionId": session, "prompt": [{"type": "text", "text": QUESTION}]},
            )
            out["after_load"] = text
            out["replayed_user_chunks"] = again.user_chunks
            out["honoured_after_load"] = text.strip().lstrip("*_` ").startswith("Marlow")
        again.close()
    else:
        agent.close()
    print(json.dumps(out))


main()
```
