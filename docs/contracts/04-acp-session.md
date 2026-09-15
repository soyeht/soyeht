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
   immediately; the result comes through notifications.
7. **`session/update`.** The engine stores everything in the pane transcript
   and forwards it to clients: message chunks, thoughts, tool calls and updates,
   plan, options changed by the agent, token usage and `state_update`. The turn
   ends when `idle` arrives with a `stopReason` (`end_turn`, `cancelled`,
   `max_tokens`, `refusal`…).
8. **`session/request_permission`.** The agent asks with a title, what it wants
   to do, and options (`allow_once`, `allow_always`, `reject_once`, `reject_always`).
   The engine responds according to §4.
9. **`session/cancel`.** Notification. The engine waits for `idle` with
   `cancelled`; if it does not arrive within 10 s, kill the process and mark the pane.
10. **Resume** (§6) and **close** (`session/close`, then terminate the process).

## 3. Prompt while the agent is busy

DECIDED (Caio, 2026-09-10): the engine queues it. If the user sends text during
a turn, the engine stores it and sends it when `idle` arrives. The pane shows
“1 queued”. With ⌘. the user cancels the current turn, and the queue is sent next.
(Only some harnesses support mid-turn “steering”; standard ACP does not promise
this, so we do not depend on it.)

## 4. Permission

The protocol has only one mechanism: the agent asks, the client responds.
“Permission mode” is an agent configuration option when offered (the Claude Code
adapter offers `default`, `acceptEdits`, `plan`, `bypassPermissions`, `dontAsk`).

Engine rule by pane mode:
- **ask** (default): the request becomes a `pane.permission_requested` event
  with title, details and options. It appears on Mac and iPhone. The first
  response wins; the engine answers the agent and notifies the other client.
  No answer within 30 min: the engine responds `cancelled`, and the pane shows
  “waiting for your permission” in the attention inbox.
- **accept edits**: automatically select `allow_once` when the request is a
  file write within the worktree; ask for everything else.
- **allow all**: respond `allow_once` to everything without asking. Also apply
  the agent's `bypassPermissions` option when available so it does not ask.
- A user's `allow_always` selection applies only to this pane (DECIDED,
  Caio 2026-09-10).
- **“yolo” button in the pane** (Caio's request): changes the pane to “allow all”
  mid-conversation. Internally, the engine starts responding `allow_once` to
  every request, and when the agent offers a mode option (e.g. `bypassPermissions`),
  applies it through `set_config_option` (contract 02) so it does not ask.
  It can be turned back off. The button stays visible in the pane toolbar
  and on the iPhone.

## 5. Capabilities the engine offers the agent

- `fs/read_text_file` and `fs/write_text_file`: yes, restricted to the pane
  worktree; refuse paths outside it with an error.
- `terminal/*` (the agent asks the client to run a command and read its output):
  **DECIDED (Caio, 2026-09-10): yes.** The command runs in a Soyeht terminal
  attached to the pane, and output appears as a terminal line within the conversation.
  This is what a terminal multiplexer does best; chat-only apps declare `terminal: false`.
- Elicitation (structured user questions): yes, becomes the same question event as permission.

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
3. DECIDED: “always allow” applies only to this pane; per-pane yolo button.
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
Rule: the engine has ONE internal lifecycle (prompt accepted → working → ended with reason) and one adapter per version. In v1, “ended” = prompt response; never wait for `idle`. On v1 cancellation, wait up to 10 s for the prompt response before killing. The first delivery implements v1 fully; v2 only when a real harness responds 2 to `initialize`.

**R04 · reconnecting to a live harness is not resuming a session.** After engine restart (contract 07), the engine↔ptyd channel delivers: harness instance ID, generation and unanswered harness requests (permission, `fs/*`, `terminal/*`) with sequence numbers. The engine answers those pending requests from persisted state; a request with an ambiguous effect (e.g. write completed, response not sent) is marked for reconciliation and never blindly repeated. On this path the engine does NOT call `session/new` or `session/resume`. `session/resume` is only for a dead process (ptyd or machine restarted).

**R12 · states.** Single enum (contract 05 §1): `starting`, `working`, `idle`, `waiting_for_you`, `cancelling`, `no_signal`, `recovering`, `dead`, `unknown`. `waiting_for_you` exists only while a permission or elicitation request is open; never derived from silence. `no_signal` is a suspicion with source and confidence; it neither releases work nor clears an open request. A pane starts in `starting` and becomes `idle` only after the first real `idle` (or first prompt response in v1).

**R14 · permission requests have identity and deadline.** Engine `requestId` (not the native ID), bound to generation and turn. The first valid response wins. On expiry (30 min), cancellation or session switch: the engine closes the request, responds `cancelled` to the harness and publishes `pane.permission_answered {result: expired|cancelled}`; the attention inbox shows “permission expired” with a “resume” action, never the old question.

**R15 · yolo, accept edits, elicitation.** Yolo: the engine stores the previous mode and the mode actually confirmed by the harness; the button shows “off” only after the harness confirms restoration (or the session restarts when the harness cannot change). Accept edits: the engine approves automatically only when the advertised option has `kind: allow_once` AND the request is recognizably a file write within the worktree; any doubt means asking. `allow_once` is the category (`kind`), not the literal ID. Elicitation (a question with fields) is a separate request with its own schema, never answered by yolo.

**R16 · switching, configuration and cancellation are serialized per pane.** During a harness switch: the prompt queue freezes; the continuation package includes the queue; the old harness is terminated only after the new one accepts the first prompt; if acceptance is ambiguous (timeout), the engine reconciles by reading the new transcript before resending. Failure: restore the old harness if alive, otherwise `session/resume`, otherwise notice + new session (`pane.harness_switch_failed {restored}`). `pane.config_changed` reports `sessionPreserved`.

**R17 · transcript = normalized messages, not raw notifications.** The engine records every accepted prompt (`user` role, `promptId`) and agent messages assembled from chunks (`assistant` role, `localId`, `generation`, `seq`, `complete`). `session/load`/`resume` replay reconciles by identity (same generation and seq) or replaces the bounded section, never duplicates. `prompts/harness-switch.en.md` is rendered only from the body after `---`; single literal substitution of each `{{field}}`; if metadata alone exceeds 16,000 characters, error (never a package without a message).

**R25 · probing in a disposable session.** The §2.3 probe runs in its own session with valid parameters, a 10 s deadline and cleanup; results are classified as `worked | unsupported | unauthenticated | inconclusive`. Only proven `unsupported` or proven session loss triggers the fallback (new session + package); timeout or a parameter error does NOT restart a conversation.

## Addendum (2026-09-11, decided by Caio after the benchmark)
**Steer.** A message queued while the agent is busy (§3) can be promoted
by the user with the "Send now" action only after a disposable fingerprint
probe has proved a reviewed harness-specific steering operation. Standard
ACP v1 does not define that operation, so slice 2 exposes no steering method
or event and disables the action with the tooltip "this agent only reads new
messages between turns". The method and `pane.queue.steered {promptId}` event
must be added together with the first proven adapter. The queue pauses by itself
when the user cancels the current turn (⌘.) and shows "queue paused because you
interrupted" with a "Resume" button; nothing is sent until the user resumes.
Slice 2 events are `pane.queue.paused` and `pane.queue.resumed`.

**Context meter.** The agent pane header shows a small ring with the
fraction of the model's context window in use, only when the harness
reports both the used and the total (`usage` update with both fields).
When either is missing, nothing is drawn: no ring, no placeholder. The
ring turns to the `warning` token above 75 % and `danger` above 90 %.
Clicking it offers "Compact context" when the harness supports it
(contract 02 capability) and otherwise shows the numbers only.
