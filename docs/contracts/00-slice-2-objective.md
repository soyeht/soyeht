# Objective · New Soyeht slice 2: the agent pane over ACP

Self-contained, for execution to completion without questions to Caio. Same
absolute safeguards as slice 1 (`00-slice-1-objective.md` §0 and §5: no
production, Dev profile `Soyeht.next/dev`, no secrets, no tags, no competitor
names, no assistant attribution, Apache-2.0). Same autonomy rule: ambiguity
becomes an ADR and work continues; self-merge with green CI; victoria reviews
afterward. Date: 2026-09-11.

## 0. Governing contracts for this slice
`04-acp-session.md` (entire document, including “Review applied”: v1/v2 table,
engine↔ptyd pipes channel, states, permission identity and deadline,
yolo/accept edits/elicitation, normalized transcript, disposable-session
probing), `02-model-effort-permission-switch.md`, `05-pane-state.md`
§§1–2 (ACP part only), `07-persistence.md` (harness under ptyd through pipes),
`08-engine-and-clients.md` (agent pane methods and events). Design:
`design/soyeht-desktop.pen`: Screen 2 (new pane, harness · model · effort
selector, permission) and Screen 5 (agent pane with chat and permission request).

## 1. What to build
1. **Protocol**: add to `contracts/protocol.json` (with vectors and generated
   types) methods `pane.prompt`, `pane.queue.list`, `pane.queue.remove`,
   `pane.cancel`, `pane.permission.answer`, `pane.elicitation.answer`,
   `pane.config.set`, `pane.yolo`, `pane.auth.start`, `pane.transcript`,
   `pane.draft`, `harness.list`, `model.list`, and `pane.create` with
   `mode: "agent"`; events `pane.session_started`, `pane.update` (closed union
   from 08 §5), `pane.turn_ended`, `pane.queued`,
   `pane.permission_requested|answered`, `pane.elicitation_requested|answered`,
   `pane.config_changing|changed|failed`, `pane.session_lost`,
   `pane.needs_login`, `pane.auth_finished`; errors `pane_busy`,
   `needs_login`, `harness_unavailable`.
2. **`soyeht-ptyd`**: second process type, `pipes` (stdin/stdout through pipes,
   stderr to a profile log file), with the SAME retention, `seq`/`ack`,
   `instanceKey`, generation and reconnection as `pty`; the engine reads
   and writes ACP JSON-RPC messages through it; on reconnection, `ptyd`
   replays unacknowledged output and the list of unanswered harness requests.
3. **`soyeht-engine`, ACP v1 client** (`soyeht-acp` as its own crate):
   launch from an ACP registry entry or configured command; `initialize`
   with `protocolVersion: 1` and client capabilities (`fs` restricted to
   the worktree; `terminal` = YES, running in a ptyd PTY and displaying output
   in the conversation; accept 2 only if the harness responds 2, otherwise
   v1 adapter); disposable-session probing by fingerprint (binary + version
   + configuration), results `worked|unsupported|unauthenticated|inconclusive`
   stored in the database; `session/new` with `cwd` and empty `mcpServers`
   in this slice; read and apply configuration options; `session/prompt`
   with turn end determined by the RESPONSE (v1); `session/update` → normalized
   transcript (accepted user messages + agent messages assembled from chunks,
   with `generation` and `seq`) → `pane.update` for clients; permission with
   its own `requestId`, 30-minute deadline, first response wins, and pane-mode
   rules (ask / accept edits / allow all); separate elicitation; `session/cancel`
   awaiting the prompt response; durable prompt queue with origin and `promptId`,
   `pane.draft` signal; yolo with previous mode and confirmation; `session/load`
   (v1) on recovery after ptyd death, otherwise a new session + `pane.session_lost`
   notice (continuation package is slice 4); `pane.needs_login` + `pane.auth.start`
   opening login in a raw ptyd terminal pane; pane state only from the protocol
   (`starting|working|idle|waiting_for_you|cancelling|no_signal|recovering|dead`),
   `no_signal` with a 90 s grace period and no effect on open requests.
4. **Test harness**: a deterministic fake ACP v1 agent in Rust inside the repo
   (`engine/soyeht-acp/tests/fake-agent`) that answers prompts, requests
   permission, accepts `set_config_option`, supports `session/load` when
   configured and can be told to “die”. CI uses only this fake. An optional
   local test (`SOYEHT_REAL_ACP=1`) uses the official Claude Code ACP adapter,
   already authenticated on Caio's Mac.
5. **CLI**: `pane create --agent <harness> [--model] [--effort]`, `pane prompt
   <id> "text"`, `pane cancel`, `pane permission <id> allow|deny`, `pane
   config <id> <key> <value>`, `pane yolo <id> on|off`, `harness list`,
   `model list <harness>`, `pane transcript <id>`.
6. **Mac app**: Screen 2 (new pane: terminal or agent mode; unified harness ·
   model · effort selector as in the `.pen`; permission; directory/worktree)
   and Screen 5 (agent pane chat: messages, collapsed thoughts, tool calls,
   inline agent terminal output, permission request with buttons, queue count,
   yolo button, cancel, footer selectors). Neomorphic style from the `.pen`.
7. **iPhone app**: read the agent pane (same view, read-only), answer permission
   requests, send text, yolo button; reconnection through snapshot.
8. **CI**: everything from slice 1 plus this slice's tests; e2e with the fake
   agent under launchd.

## 2. Acceptance (work ends only when all pass)
1. [CI] Open an agent pane, send a prompt: response arrives in chunks and appears
   identically on Mac and iPhone (two connected clients); persisted, paginated transcript.
2. [CI] Permission: the request appears on both clients; the first response wins;
   the other receives `permission_answered {by}`; no answer within 30 min
   (accelerated in the test) → `expired` and attention inbox.
3. [CI] Cancel mid-turn: prompt response with `cancelled` and pane `idle` within
   10 s; without killing a healthy process.
4. [CI] `kill -9` the engine mid-turn: harness continues under ptyd (same PID),
   engine adopts it, subsequent chunks arrive, and a permission request made
   during the outage is recovered and answered exactly once.
5. [CI] Change model and effort mid-conversation: `config_changing` →
   `config_changed {sessionPreserved: true}`; a parameter error does NOT restart the session.
6. [CI] Yolo turns on and off with harness confirmation; permission requested
   while yolo is on is approved without appearing to the user; elicitation is
   NEVER answered by yolo.
7. [CI] A prompt sent during a turn stays in the queue (`pane.queued`) and is sent
   after `idle`; with `pane.draft` active, an automatic prompt does not enter.
8. [CI] Force-restart ptyd: harness dies; engine attempts `session/load`; fake
   agent configured with support → transcript continues; without support → new
   session + `session_lost` notice; never silently.
9. [CI] Probing: a fake agent that “advertises” resume but returns `method_not_found`
   is recorded as `unsupported` and not probed again until its version changes.
10. [CI] Login: fake agent returns `auth_required` → `pane.needs_login`;
    `pane.auth.start` opens the terminal pane; then `pane.auth_finished`
    and the session opens.
11. [local, procedure] Same flows with the official Claude Code ACP adapter on
    Caio's Mac: real prompt, real permission answered from iPhone with Wi-Fi
    disabled, model change, yolo, `kill -9` the engine mid-turn.

## 3. Outside this slice
Tasks, loop, lead agent, Soyeht MCP within the session (slice 3); continuation
package and harness switching (slice 4); hooks/regex in raw terminals
(slice 5); push (slice 6); ACP v2 (only when a real harness responds 2).

## 4. Workflow and delivery
Same as slice 1: order protocol → ptyd pipes → ACP client + fake agent →
engine → CLI → e2e → Mac app → iPhone app → procedure; small PRs, green CI,
self-merge; an ADR for each ambiguity; on completion, `docs/reports/slice-2.md`
and a message to victoria with PR links.
