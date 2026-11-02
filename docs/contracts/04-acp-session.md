# Contract 04 · Session with a harness over ACP

Status: SETTLED on 2026-09-10 (Caio's four decisions at the end).
Source: (internal analysis, outside the repository) (protocol and registry),
Claude Code ACP adapter, and how internally analyzed tools handle permission,
cancellation and recovery. Date: 2026-09-10. No names of other products.

## 1. What the contract promises

An agent-mode pane is an ACP session opened, fed and observed by the engine.
The engine is the client; the harness is a child process speaking line-delimited
JSON. Mac, Linux/Windows and iPhone never speak ACP; they receive engine events.
Raw terminal panes continue to exist and do not use this path.

## 2. Lifecycle, step by step

1. **Launch.** The engine reads the harness registry entry (command through
   npx, uvx or binary, with a hash when available) or user configuration, and
   opens the process with stdin/stdout pipes in the pane worktree's `cwd`.
2. **`initialize`.** The engine sends `protocolVersion: 1` first (what the
   ecosystem answers today); if the agent responds 2, the engine speaks 2.
   It sends client capabilities (see §5) and the name “Soyeht”.
   It reads `authMethods`; if login is required, see §7.
3. **Probe.** Agent advertisements cannot be trusted. After `initialize`,
   the engine tests `session/list`, `session/resume` (or `session/load` in v1)
   and `session/set_config_option` once per fingerprint (harness + binary
   version + configuration), and stores actual results. It repeats when the
   fingerprint changes. Even with a stored result, if the operation fails at
   runtime, the engine applies §6's fallback (new session + notice). Never fails silently.
4. **`session/new`** with `cwd`. In slice 2, `mcpServers` is an empty list.
   Slice 3 adds Soyeht's own MCP (task tools, pane messaging, etc.) to this
   list so every ACP agent receives those tools without manual configuration.
   Store `sessionId` + `cwd` + harness + version in pane state.
5. **Options.** Read `configOptions` from the response (permission mode,
   model, effort) and apply the pane or task's requested values through
   `session/set_config_option` (contract 02).
6. **`session/prompt`.** User text, the task's first prompt, or the continuation
   package (contract 01), as content blocks. The agent responds “accepted”
   immediately; the result comes through notifications. Files attached to the
   prompt follow the text, one block each, in the richest form the agent's
   `initialize` `promptCapabilities` allow: `image` for PNG, JPEG, GIF and WebP,
   `audio`, an embedded text `resource`, and otherwise a `resource_link` to the
   file's absolute path, which every ACP v1 agent accepts. A file that would
   push the message past one ACP line is linked (ADR 0076).
7. **`session/update`.** The engine stores everything in the pane transcript
   and forwards it to clients: message chunks, thoughts, tool calls and updates,
   plan, options changed by the agent, token usage and `state_update`. The turn
   ends when `idle` arrives with a `stopReason` (`end_turn`, `cancelled`,
   `max_tokens`, `refusal`…).

   **A refused model.** A turn that fails ends with `stopReason: error` and a
   `system` message that says why. When the provider refuses the pane's model, the
   message names it (`The agent refused model "M": reason`) and `model.list`
   keeps the refusal as `observedRefusal` of that model. Adapters report the
   refusal as a protocol error, or only as the text of a reply. The engine takes
   the second form for a refusal only when the turn wrote nothing but that one
   reply (besides thoughts and usage) and the reply is exactly the provider's
   sentence for the pane's selected model (`There's an issue with the selected
   model (ID). It may not exist or you may not have access to it…`, ID equal to
   the model's id); a reply that quotes or explains the sentence is an answer. It
   then ends that turn with `error` instead of `end_turn`. The engine never picks a model: a pane that names none uses the
   agent's own current value, so a default the account cannot use comes from the
   agent and shows as this refusal.

   **Subagents.** A harness can run subagents inside the turn. The engine
   follows the ACP subagent draft (agentclientprotocol/agent-client-protocol#1992),
   which the pinned Claude and Codex adapters implement: the parent session
   reports `subagent_spawned {subagentSessionId, name, task}` and
   `subagent_state_update {subagentSessionId, state}`, and each subagent's
   own updates arrive under its `subagentSessionId`. Each subagent becomes one
   transcript item, `kind: subagent`, whose `subagent` holds
   `{subagentId, name, task, status, model?, startedAt?, finishedAt?, summary?}`;
   `status` is `running` until the state update (`completed`, `failed`,
   `cancelled`, `disconnected`). The subagent's messages, thoughts, tool calls
   and plans are stored with `subagentId` and are never presented as the parent
   agent's own; a subagent that spawns another nests the same way. A subagent's
   usage, mode, configuration and prompt echo are not the pane's and are
   dropped. `summary` previews the subagent's last message when it finishes.
   `model` is set only when the harness reports it: Claude Code reports it as
   `_meta.claudeCode.toolResponse.resolvedModel` on the spawning Agent call,
   whose `agentId` names the subagent session; that call updates the row and
   never becomes a tool row. Codex reports no model. Codex's collaboration
   `wait` calls (`_meta.codex.collaboration.tool: "wait"`) on subagents the
   pane tracks never become tool rows either: an empty `receiverThreadIds`
   counts as tied only while the pane tracks a subagent, and a wait naming a
   session it does not track stays a tool row. Clients show a waiting status
   while the transcript ends in running subagent rows. Times are wall-clock
   times the engine observed live; a replay leaves them absent. Permission,
   file and terminal requests from a reported subagent session are accepted
   like the parent's; a subagent's permission request is presented from the
   tool call that subagent reported, and a persistent grant made for a
   subagent's request never covers the parent's requests, nor the reverse.
   When the harness process ends, subagents still running
   are marked `disconnected`. An engine restart keeps the stored rows; a native
   `session/load` replaces them with what the adapter replays. Codex replays
   its subagents; Claude Code (pinned 0.77) replays neither the subagents nor
   their Agent calls, so after its process restarts only the parent's
   messages return.
8. **`session/request_permission`.** The agent asks with a title, what it wants
   to do, and options (`allow_once`, `allow_always`, `reject_once`, `reject_always`).
   The engine responds according to §4.
9. **`session/cancel`.** Notification. The engine waits for `idle` with
   `cancelled`; if it does not arrive within 10 s, kill the process and mark the pane. The engine
   sends `session/cancel` again every second until the prompt answers or the 10 s deadline passes,
   since an agent drops a cancel that reaches it before its turn has begun.
10. **Resume** (§6) and **close** (`session/close`, then terminate the process).

## 3. Prompt while the agent is busy

DECIDED (Caio, 2026-09-10): the engine queues it. If the user sends text during
a turn, the engine stores it and sends it when `idle` arrives. The pane shows
“1 queued”. With ⌘. the user cancels the current turn, and the queue is sent next.
(Only some harnesses support mid-turn “steering”; standard ACP does not promise
this, so we do not depend on it.)

## 4. Permission

The protocol has only one mechanism: the agent asks, the client responds.
The permission mode is the harness's own mode selector (ADR 0070, which
replaced the Yolo toggle on 2026-09-30). The Claude adapter announces
`default`, `acceptEdits`, `plan`, `auto` and `bypassPermissions`; the Codex
adapter announces `read-only`, `agent` and `agent-full-access`. The pane menu
shows those names; the harness decides which actions it asks about.

- Every request the agent sends becomes a `pane.permission_requested` event
  with title, details and options. It appears on Mac, iPhone and iPad. The
  first response wins; the engine answers the agent and notifies the other
  clients. No answer within 30 min: the engine responds `cancelled`, and the
  pane shows “waiting for your permission” in the attention inbox.
- An MCP tool request is shown as “Use <server> · <tool>” with each argument
  in full, whichever harness sends it (codex-acp: kind `execute` with
  `rawInput {server, tool, arguments}`; claude-acp: tool name
  `mcp__<server>__<tool>`). Pane ids of the same workspace show as pane names.
  codex-acp 1.11.0 sends only the tool call id, kind and status in the
  request; the engine judges every request on the tool call as last reported
  by `session/update` in the same prompt, overlaid with the request's own
  fields. An `execute` request with neither a command nor this shape is refused.
- A user's `allow_always` selection applies only to this pane (DECIDED,
  Caio 2026-09-10), and only to a request that is the same and is shown the
  same way (title, command, arguments and changes).
- Only an agent that announces no modes gets the engine's own `ask`,
  `accept_edits` and `allow_all` modes, enforced by answering its requests
  (`accept_edits`: `allow_once` for a file write within the worktree).

## 5. Capabilities the engine offers the agent

- `fs/read_text_file` and `fs/write_text_file`: yes, restricted to the pane
  worktree; refuse paths outside it with an error.
- `terminal/*` (the agent asks the client to run a command and read its output):
  **DECIDED (Caio, 2026-09-10): yes.** The command runs in a Soyeht terminal
  attached to the pane, and output appears as a terminal line within the conversation.
  This is what a terminal multiplexer does best; chat-only apps declare `terminal: false`.
- Elicitation (structured user questions): yes, becomes the same question event as permission.
- Subagents: yes, as `subagents: {}` (the ACP draft) and as the JetBrains AIR
  `_meta.jetbrains.air` capability `nativeSubagentSessions`. The published ACP
  TypeScript SDK drops the draft field while parsing `initialize`, so the
  pinned adapters only see the `_meta` form; without it they flatten subagent
  tool calls into the parent session (Claude) or omit subagent activity
  (Codex).

## 6. Resume after restart

The engine stores `sessionId` + `cwd` + harness + version. On startup:
- v2: `session/resume` without replay (the engine already has the transcript);
  request `replayFrom: start` only if the local transcript is corrupted.
- v1: `session/load` with the ID.
- On failure (method missing, ID not found, agent updated): `session/new` +
  contract 01's continuation package + system comment “native session lost;
  continuing with the latest messages”. Never fails silently.

## 7. Login

`authMethods` from `initialize`: `terminal` type = the engine opens the same
binary with login arguments in a raw terminal pane for the user to authenticate,
then relaunches the agent; `agent` type = `auth/login` through the protocol.
Until logged in, the pane shows “login required” with a button.

An `agent` method that the person pressed is remembered for the harness, durably. Some
agents ask every process for it, so the engine sends the same `authenticate` request to each
new process of any pane of that harness, and to the candidate of a switch into it, before the
capability probe or the session (ADR 0098, ADR 0100). The method is sent only while the agent
still offers it as an `agent` method. A refusal, and signing out, forget it and the pane asks
again. Only the method id is stored.

A zero exit status from the login terminal is not proof of sign-in. The engine
closes that terminal, with `pane.auth_finished {ok: true}`, only after the
relaunched agent opens a session. If the command fails, is interrupted, or exits 0
while the agent still requires sign-in, the terminal and its output stay open for
review and `pane.auth_finished {ok: false, reason}` says what happened.

The login terminal is a plain supervisor-owned process. Only three things end it:
the person finishes it, the person cancels it (`pane.auth.cancel`, which closes
the terminal and its process) or closes its terminal, or the supervisor itself
stops. A second `pane.auth.start` while it runs returns the same terminal. The
agent process dying during a login does not end the login: when the login exits
0 the agent is restarted, whatever state it was in. The agent pane publishes the
state in `agent.login {phase: waiting|verifying|signing_out, terminalPaneId?}`
so every client shows a running sign-in, a sign-in being checked and a sign-out
in progress. Every ending of a sign-in without a sign-in says why in
`pane.auth_finished.reason`: failed or interrupted, terminal closed, cancelled,
or lost because the supervisor restarted. The engine writes an
`engine.sign_in_ended` log record with the exit code and cause.

### Waiting for sign-in

A pane that needs sign-in keeps the person's prompts. The prompts of the person
that cannot start are written to the transcript at once, followed by a `system`
message that says the agent waits for sign-in and that sending another prompt
checks again (one message for each batch of prompts recorded together). A queued
prompt is written once: starting it later does not repeat it. A reopened session
that is loaded replaces the transcript with the agent's replay, so a prompt that
had been written but did not start is written again when it starts. A prompt of
the person sent to such a pane, with no sign-in running, restarts the agent so
that it reports whether it still needs a sign-in; an automatic prompt only waits.
A sign-in repaired outside Soyeht (the person signed in with the agent's own
command) therefore recovers on the next prompt, without closing the pane, and
the queued prompts run in order. While a sign-in runs, a prompt only waits.

### Switching account and signing out

A pane that is signed in offers the same flow as an account switch:
`agent.reauthMethods` lists the terminal methods and `pane.auth.start` accepts
them. The running session is untouched while the person is in the browser. A
successful login restarts the agent, which reopens the same session with
`session/load`, so the history stays and the new account is used. A failed,
cancelled or lost switch leaves the session and its account as they were. The
engine refuses a switch or sign-out while a turn or configuration change runs
(`pane_busy`).

An agent that advertises `agentCapabilities.auth.logout` also gets
`agent.canSignOut` and `pane.auth.logout`: the engine sends ACP `logout`, then
restarts the agent, which reports that it needs a sign-in and offers the methods
again. The session id is kept for the next sign-in.

## 8. Derived pane state (input to contract 05)

`working` = between prompt acceptance and `idle`. From prompt acceptance until the
first parsed turn output, the pane quietly shows the durable elapsed wait. After
`noSignalSec` (90 seconds), `no_signal` escalates the presentation
without ending the turn. Parsed message/thought, tool, plan, agent request or final
response permanently clears the wait for that turn; a long turn that has started
streaming remains healthy. Usage, configuration, mode and unknown notifications do
not clear the wait: a three-minute capture of the observed wedge emitted catalogue
and accounting updates without producing turn output. This errs toward a
non-destructive notice; no automatic cancel or restart may be attached to it.
The parsed protocol activity decides the state; the clock decides only when the
same state is presented more urgently. An explicit permission or elicitation
request instead enters `waiting_for_you` and renders its question card, so a turn
blocked on the person is never reported as merely quiet.
Wall-clock sleep remains part of the elapsed wait because the prompt stayed active;
a backward clock jump clamps the display to zero. The person may keep waiting or
cancel; silence never cancels automatically. The same `noSignalSec` threshold has
one meaning in both paths: the engine cannot confirm useful progress. Without an
active prompt that means no protocol signal; during a prompt it means no parsed
turn output, even when irrelevant protocol noise continues. “waiting for permission”
= open request; `idle` = `idle`; “cancelling”; “no signal” = live process,
open turn, no parsed turn activity for N s; `dead` = process exited.
`stopReason` and the last message feed the card's state phrase.

## 9. Client events

`pane.session_started {harness, version, options}`, `pane.update {...}` (one
per `session/update` type), `pane.turn_ended {stopReason}`,
`pane.permission_requested {requestId, title, options}`, `pane.permission_answered
{requestId, by: mac|iphone|rule}`, `pane.queued {n}`, `pane.queue.paused`,
`pane.queue.resumed`, `pane.session_lost {reason}`,
`pane.needs_login`.

## 10. Excluded scope

Raw terminal (another contract); `session/fork` (unstable); HTTP transport
(protocol draft); engine as an ACP server for clients (noted, not decided).

## Pending closed-choice questions

1. DECIDED: yes, the command appears and runs inside the pane.
2. DECIDED: queue.
3. DECIDED: “always allow” applies only to this pane; per-pane mode menu
   with the harness's own modes (ADR 0070).
4. DECIDED: probe once per binary version and store; runtime failure uses
   the fallback (new session + notice).

## Review applied (2026-09-10, findings R03, R04, R12, R14, R15, R16, R17, R25)

**R03 · v1 and v2 have different lifecycles; the engine normalizes both.**
| | ACP v1 (what the ecosystem speaks today) | ACP v2 |
|---|---|---|
| turn end | the `session/prompt` RESPONSE arrives at the end and carries `stopReason` | immediate empty response; end comes through a `session/update` state `idle` with `stopReason` |
| cancel | `session/cancel`; prompt response arrives with `stopReason: cancelled` | same, but `idle`/`cancelled` arrives through an update |
| resume | `session/load {sessionId, cwd}` (replays history through updates) | `session/resume {sessionId, cwd, replayFrom?}` |
| permission mode | `session/set_mode` OR configuration option | configuration option only |
| agent-requested files and terminal | `fs/*`, `terminal/*` exist | removed from the core (become extensions) |
| login | `authenticate` | `auth/login` |
Rule: the engine has ONE internal lifecycle (prompt accepted → working → ended with reason) and one adapter per version. In v1, “ended” = prompt response; never wait for `idle`. On v1 cancellation, wait up to 10 s for the prompt response before killing, sending `session/cancel` again every second meanwhile. The first delivery implements v1 fully; v2 only when a real harness responds 2 to `initialize`.

**R04 · reconnecting to a live harness is not resuming a session.** After engine restart (contract 07), the engine↔ptyd channel delivers: harness instance ID, generation and unanswered harness requests (permission, `fs/*`, `terminal/*`) with sequence numbers. The engine answers those pending requests from persisted state; a request with an ambiguous effect (e.g. write completed, response not sent) is marked for reconciliation and never blindly repeated. On this path the engine does NOT call `session/new` or `session/resume`. `session/resume` is only for a dead process (ptyd or machine restarted).

**R12 · states.** Single enum (contract 05 §1): `starting`, `working`, `idle`, `waiting_for_you`, `cancelling`, `no_signal`, `recovering`, `dead`, `unknown`. `waiting_for_you` exists only while a permission or elicitation request is open; never derived from silence. `no_signal` is a suspicion with source and confidence; it neither releases work nor clears an open request. A pane starts in `starting` and becomes `idle` once its agent session is open (`pane.session_started`) and no turn runs; it does not wait for a first turn.

**R14 · permission requests have identity and deadline.** Engine `requestId` (not the native ID), bound to generation and turn. The first valid response wins. On expiry (30 min), cancellation or session switch: the engine closes the request, responds `cancelled` to the harness and publishes `pane.permission_answered {result: expired|cancelled}`; the attention inbox shows “permission expired” with a “resume” action, never the old question.

**R15 · modes, accept edits, elicitation.** The mode menu shows a value only after the harness confirms it (ADR 0070). The engine's own accept edits, used only for agents without native modes, approves automatically only when the advertised option has `kind: allow_once` AND the request is recognizably a file write within the worktree; any doubt means asking. `allow_once` is the category (`kind`), not the literal ID. Elicitation (a question with fields) is a separate request with its own schema, never answered by any mode.

**R16 · switching, configuration and cancellation are serialized per pane.** During a harness switch: the prompt queue freezes; the continuation package includes the queue; the old harness is terminated only after the new one accepts the first prompt; if acceptance is ambiguous (timeout), the engine reconciles by reading the new transcript before resending. Failure: restore the old harness if alive, otherwise `session/resume`, otherwise notice + new session (`pane.harness_switch_failed {restored}`). `pane.config_changed` reports `sessionPreserved`.

**R17 · transcript = normalized messages, not raw notifications.** The engine records every accepted prompt (`user` role, `promptId`) and agent messages assembled from chunks (`assistant` role, `localId`, `generation`, `seq`, `complete`). A prompt queued by another agent pane through `agent.message` keeps its sender as `agentMessage {paneId, name, harness}` in the queue and the transcript; clients label the bubble with the sender's name. Only the prompt sent to the harness adds the reply route (the sender's pane identifier and the pane's own `soyeht-workspace` MCP server, a name distinct from any user-configured Soyeht server), so no bubble shows it; a native replay of that prompt is folded back into the attributed form. `session/load`/`resume` replay reconciles by identity (same generation and seq) or replaces the bounded section, never duplicates. `prompts/harness-switch.en.md` is rendered only from the body after `---`; single literal substitution of each `{{field}}`; if metadata alone exceeds 16,000 characters, error (never a package without a message).

**R25 · probing in a disposable session.** The §2.3 probe runs in its own session with valid parameters, a 10 s deadline and cleanup; results are classified as `worked | unsupported | unauthenticated | inconclusive`. Only proven `unsupported` or proven session loss triggers the fallback (new session + package); timeout or a parameter error does NOT restart a conversation.

## Addendum (2026-09-11, decided by Caio after the benchmark)
**Steer.** A message queued while the agent is busy (§3) can be promoted
by the user with the "Send now" action only after a disposable fingerprint
probe has proved a reviewed harness-specific steering operation. Standard
ACP v1 does not define that operation, so slice 2 exposes no steering method
or event and disables the action with the tooltip "this agent only reads new
messages between turns". The method and its `{promptId}` event
must be added together with the first proven adapter. The queue pauses by itself
when the user cancels the current turn (⌘.) and shows "queue paused because you
interrupted" with a "Resume" button; nothing is sent until the user resumes. A
cancel while no turn runs (`cancelling: false`) interrupts nothing and does not pause
the queue.
Slice 2 events are `pane.queue.paused` and `pane.queue.resumed`.

**Context meter.** The agent pane header shows a small ring with the
fraction of the model's context window in use, only when the harness
reports both the used and the total (`usage` update with both fields).
When either is missing, nothing is drawn: no ring, no placeholder. The
ring turns to the `warning` token above 75 % and `danger` above 90 %.
Clicking it offers "Compact context" when the harness supports it
(contract 02 capability) and otherwise shows the numbers only.
