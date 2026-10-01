# Contract 08 · Engine communication with clients

Status: v2 CLEARED FOR SLICE 1 by aria's review on 2026-09-10 (“slice 1 cleared”);
the rest of v1 remains open to adjustments. Rewritten after aria's review
(internal contract review, findings R01, R02, R05, R06, R10, R11, R14, R18, R19,
R20 applied here). Clients: new Swift app (Mac and iPhone), TypeScript app
(Linux and Windows), CLI and agents' MCP server.

## 1. Unchanging rules
1. **One contract** for every client. The iPhone is the same client with a
   different screen. CLI and MCP server are clients too.
2. **Clients never calculate state.** Request the snapshot, receive events, render.
3. **Complete JSON-RPC 2.0 envelope** (R19): request `{jsonrpc, id, method,
   params}`, response `{jsonrpc, id, result}` or `{jsonrpc, id, error}`,
   event = notification `{jsonrpc, method, params}` without `id`. The event
   sequence number goes INSIDE `params.eventId`. Errors have numeric `code`
   (range -32000..-32099 for ours) and `data.kind` with a stable name.
4. **Every frame on the local socket has a common header** (R06):
   `[type u8][size u32][body]`. Type 1 = JSON (UTF-8), type 2 = terminal
   bytes (§6), type 3 = attachment (§8). Size covers only the body.
   **All binary integers in this contract are big-endian**, in header and body.
   On WebSocket: text frame = JSON; binary frame = types 2 and 3, with the same
   5-byte header inside the frame. Limits: type 1 body ≤ 1 MiB (except
   `state.snapshot`, ≤ 16 MiB); type 2 body = 16-byte subheader + up to 64 KiB
   terminal bytes; type 3 body ≤ 256 KiB + subheader.
5. **Types**: IDs are strings; 64-bit integers (offsets, sizes) travel as decimal
   strings; enums are closed strings; readers ignore unknown fields
   (allowing version N+1 to speak with N).
6. **Protocol version is an integer.** The engine accepts current and previous
   versions; clients specify theirs in `hello`; the engine responds with the
   version it will use or closes with `protocol_version_unsupported`.
7. **One schema per method and event** (`contracts/protocol.json` in the
   repository, single source; generated Rust, Swift and TypeScript types;
   CI fails if code diverges). This contract describes; the schema defines.
   Minimum slice 1 schema: IDs for all targets (`workspaceId`, `paneId`,
   `streamId`, `deviceId`, `requestId`, `promptId`) and the `result` type
   of every method; round-trip test vectors for each message and binary header.
8. **English error text with stable codes**; translation belongs to the app.

## 2. Transport, authentication and roles (R10, R11)
| Client | Path | Identity proof |
|---|---|---|
| Mac (Swift), Linux/Windows (TS), CLI | Unix socket at `<data>/engine.sock`; directory 0700, socket 0600; on Windows, named pipe with user ACL | same OS user + `hello` |
| MCP server (one per agent pane) | same local socket | `hello` with `binding` issued by the engine when creating the pane (short secret + paneId); without a valid `binding`, becomes a “local human” client without a role |
| iPhone | plaintext WebSocket on the Soyeht port at Tailscale addresses (contract 06) | `deviceToken` issued during pairing (§2.2) |

Declared limit: arbitrary processes belonging to the same user can open the
local socket; isolation is per OS user, not per process.

**2.1 Handshake (one request, one response).**
1. On accepting a connection, the engine sends `auth.challenge
   {nonce (32 random bytes, base64, unique per connection), engineId}`
   before any other message.
2. The client sends `hello {client, clientVersion, protocolVersion,
   deviceToken?, proof?, binding?}`. For iPhone,
   `proof = Ed25519(device private key, bytes = "soyeht-hello-v1"
   || nonce || engineId || deviceToken)`. Local clients send no `proof`
   (identity is the OS user); MCP sends `binding`.
3. The engine responds ONCE: `welcome {engineVersion, protocolVersion,
   connectionId, role, capabilities, dbId}` or an error (`unauthorized`,
   `protocol_version_unsupported`) and closes.
No other request is accepted before `welcome` (`unauthorized` response and
connection closed). Without valid `hello` within 5 s, the engine closes.
`role` ∈ `human_local | human_device | agent | chief`. The pairing dialogue
(§2.2) uses `pair.*` instead of `hello` and follows the code's deadline.

**2.2 iPhone pairing (the path to a `deviceToken`).**
Only on the WebSocket connection, before authenticated `hello`, and only
while the Mac's “Add iPhone” window is open:
1. Mac: `device.pair.start {}` → `{code, expiresAt (10 min), uri}`; the app
   displays a QR with `uri` (address + `code`).
2. iPhone: `pair.begin {code, deviceName, devicePublicKey (Ed25519, 32
   bytes, base64)}` → `{requestId, challenge (32 random bytes, base64)}`.
3. iPhone: `pair.prove {requestId, proof}` with `proof = HMAC-SHA-256(key,
   challenge || deviceName)`, `key = SHA-256(code || devicePublicKey)` →
   `{status: "pending"}`; Mac receives `device.pair_requested {requestId,
   deviceName, fingerprint = SHA-256(devicePublicKey) in short hex}`.
4. Mac: `device.pair.decide {requestId, approve: true|false}`. The engine waits
   for consent until the code deadline (10 min); this pre-pairing dialogue
   is NOT subject to `hello`'s 5 s limit.
5. iPhone receives `pair.result {requestId, approved, deviceToken?, deviceId?,
   reason?}`; code consumed (single use). `deviceToken` is bound to
   `devicePublicKey`: every subsequent connection's `hello` carries `proof`
   over `auth.challenge`'s `nonce` (§2.1), and the engine verifies it against
   the recorded key. After approved `pair.result`, the iPhone closes and
   reconnects with the normal handshake. Refusal reasons: `expired`,
   `consumed`, `bad_proof`, `denied`, `revoked`.
`device.revoke {deviceId}` invalidates the token, closes open connections and
removes that device's push tokens.

**2.3 Push.** `push.register {platform: "apns", token, kind: "alert" |
"live_activity", activityId?}` and `push.unregister`. The engine stores them
per device; renewal = register again. Push-triggering events:
`pane.permission_requested`, `task.moved` to “your turn”,
`pane.state_changed` to `no_signal`, `pane.session_lost`.

**2.4 Role matrix** (who may do what):

A paired device (`human_device`, iPhone or iPad) is a full remote control of
the Mac by owner decision on 2026-09-26: it has exactly the powers of
`human_local`. Pairing is what protects the Mac: a device must be approved on
the Mac (§2.2) and sign every `hello` with its key (§2.1); an unpaired, unsigned
or revoked connection gets nothing. Two differences are about where code runs,
not about permission: only the local Mac app may register as the web host,
and pane-state experiment data is recorded only in the signed Mac app.

| Operation | human_local | human_device (iPhone, iPad) | agent (pane MCP) | chief |
|---|---|---|---|---|
| view everything in the workspace | yes | yes | own task/pane only | yes |
| `pane.input` (type into raw terminal) | yes | yes | no | no |
| `pane.prompt`, `pane.cancel`, permission, mode, config | yes | yes | no | no |
| `task.create/comment/subitem/block/release/request_review/complete` | yes | yes | own task only | yes |
| `task.next` | no | no | yes | no |
| `task.dispatch`, `task.unblock`, `task.list` | yes | yes | no | yes |
| `pane.role.set` (appoint lead) | yes | yes | no | no |
| `device.*`, `settings.set`, `harness.custom.*` | yes | yes | no | no |
| `task.worktree.clean`, closing web panes | yes | yes | no (web close: own workspace) | no (web close: own workspace) |

## 3. State snapshot and events without loss (R05)
- **State change and event originate in the same database transaction.** Every
  event has an increasing `eventId`, unique per `dbId`.
- `state.snapshot {}` (global in v1; no workspace filter) → snapshot and
  `cursor` are read **in the same read transaction**; `cursor` is the last
  included `eventId`. The snapshot contains: workspaces; panes with mode,
  harness, session (`sessionId`, `generation`), state + confidence + phrase,
  configuration options and pending operation, prompt queue (IDs and origin),
  loop and counters, role; tasks with state, badges, blocking, owner, active
  attempt, subitems, last 20 comments and `commentCount`; open permission/
  elicitation requests with deadline; attention inbox; devices; effective
  per-harness capabilities; `streams` (§6). Workspaces and panes are listed
  in creation order, the order in which live `created` events append them.
- `events.subscribe {since}` (exclusive `since`): the engine **registers the
  subscription before** reading the replay boundary, sends events
  `(since, boundary]` in order, then new ones; events arriving in the live queue
  with `eventId ≤ boundary` are discarded from that queue (already replayed),
  without gaps or repetition. If `since` is older than the retained log
  (10,000 events or 7 days), return `cursor_too_old` BEFORE sending any event;
  the client repeats `snapshot`. A different `dbId` from the previous `welcome`
  = replaced database = redo everything.
- **Reconnection**: `hello` → `state.snapshot` → `events.subscribe {since:
  cursor}`. Always all three.
- Every state-changing request stores `actor = {role, client, deviceId?,
  paneId?}`; this is the card's “who moved it”.
- **Safe retries (R09)**: every state-changing method REQUIRES
  `idempotencyKey` (client UUID, valid for 24 h), scoped by stable client
  identity (`deviceId` on iPhone; OS user + client name locally). Key and
  result are stored in the SAME transaction as the state change. Same key
  and content = same response; same key with different content =
  `idempotency_conflict`. This also applies to `pane.create` and `pane.close`
  through to `ptyd` (contract 07): crashing after spawn and before recording
  does not create a second pane; a lost `close` does not resurrect a process.
- **Slow client (R20)**: output queue per connection limited to 8 MiB;
  control events take priority over terminal bytes; on overflow, the engine
  closes with `slow_consumer` and the client resynchronizes. Never silently
  discard an event; never block persistence.

## 4. Requests (methods)
Each method has its own `params` and `result` schema (never only `{ok}`
for queries). Paginated lists: `{items, nextCursor?}` with `limit`.

**Workspaces**: `workspace.list`, `workspace.create {name, path}`,
`workspace.rename {workspaceId, name}`, `workspace.close`, `workspace.focus`
(focus is **per client**, not global: tapping on iPhone does not move the Mac keyboard).

**Panes**: `pane.create {workspaceId, mode: "terminal"|"agent"|"editor", harness?,
model?, effort?, permission?, cwd|worktree?, taskId?, name?}` →
`{paneId, binding?}`; `pane.close`, `pane.rename`, `pane.arrange
{workspaceId, layout}` (layout is shared and changes only through this request),
`pane.focus` (per client), `pane.resize {cols, rows}` (policy: the engine uses
size from the last human client to resize), `pane.input {bytes}` (raw terminal),
`pane.prompt {text, attachments?, idempotencyKey}` → `{promptId, queued}`,
`pane.queue.list`, `pane.queue.remove {promptId}`,
`pane.cancel`,
`pane.permission.answer {requestId, optionId}`,
`pane.elicitation.answer {requestId, fields}` (questions with fields; never
answered by any permission mode),
`pane.config.set {configId, value}` → `{applied: bool, effective, sessionPreserved: bool}`,
`pane.switch_harness {harness, model?, effort?}` → `{switchId}`,
`loop.set {paneId, prompt, interval, maxRounds, stopWhen, enabled}`,
`pane.role.set {role: "chief"|"worker"}`,
`pane.auth.start` → opens login (contract 04 §7); event `pane.auth_finished`,
`pane.transcript {before?, limit}` → normalized messages with `role,
localId, generation, seq, complete`,
`pane.stream.subscribe {paneId, from: "tail" | {streamId, offset}}` /
`pane.stream.unsubscribe` (§6),
`pane.message.send {toPaneId, text}` and `pane.message.list` (persisted pane-to-pane
messages with `queued|delivered|read` state),
`pane.record.set {on}` (do not record this pane's screen; contract 07 §2).

**Tasks** (1:1 map to contract 03 §11 MCP tools):
`task.create`, `task.list {workspaceId, state?, limit, cursor?}`,
`task.show {taskId}` (includes history and attempts), `task.comments
{taskId, before?, limit}`, `task.update {taskId, version, ...}` (editing
protected by `version`; conflict = `stale_version`), `task.move {taskId,
state}`, `task.comment`, `task.subitem.add`, `task.subitem.done`,
`task.block {kind, reason}`, `task.release`, `task.request_review`,
`task.complete {summary, contractChanges, outOfScopeEdits}` (both lists are
required even when empty), `task.dispatch {taskId, preset}`, `task.unblock`,
`task.worktree.clean`, **`task.next {}`** (`agent` role only; rules in 03 §5),
and `loop.set {paneId, prompt, interval, maxRounds, stopWhen, enabled}`.
Task IDs are always `{workspaceId, taskId}` together.

**Files and editor**: `file.open`, `file.read`, `file.write`, `file.watch`,
`file.history`, `file.revert`, `file.undo`, `file.redo`, `file.search`, `file.symbols`,
`file.definition`, `file.references`, `file.diagnostics`, `file.diff`, and
`file.format`; `editor.review.accept|reject|comment`; `lsp.status`. Content is
read through bounded line viewports. Positions are zero-based line and UTF-16
column pairs. Mutations name a base revision when they change existing text;
the complete shapes and limits are generated from `contracts/protocol.json`.

**Experiment (contract 05 §4)**: `experiment.record.set {paneId, on}`,
`experiment.export {from, to}` → `{path, bytes}`, `experiment.metrics`,
`experiment.detector.set {name}`.

**Other**: `attention.list`, `harness.list` (registry + detected + probed
capabilities), `model.list {harness}` (session catalog when available, with
closed source `not_probed | native_session`),
`device.pair.start|decide`, `device.list`, `device.revoke`,
`push.register|unregister`, `settings.get`/`settings.set {key, value}`
(schema-typed keys), `engine.status` → `{version, generation,
ptyd: {connected, version, generation}, db: {id, schemaVersion}}`.

## 5. Events (notifications)
Names **match those published in contracts 01–04 and 07** (R01); fields are
English in every contract (mapping table at the end). Durable state events
carry `eventId`, `at`, `actor?`. Notifications declared non-durable in the
executable schema carry no state cursor and never enter replay.
- `workspace.created|renamed|closed`
- `pane.created|closed|renamed|arranged`
- `pane.session_started {harness, version, sessionId, generation, options}`
- `pane.state_changed {state, confidence, phrase, source}` (single enum from 05)
- `pane.update {kind, ...}` (closed union: `agent_message_chunk`,
  `agent_thought_chunk`, `tool_call`, `tool_call_update`, `plan`,
  `usage`, `terminal_output {terminalId, bytes}`, `config_option_update`,
  `subagent {subagent}`); message, thought, tool, plan and subagent updates
  carry `subagentId` when a harness subagent owns them (contract 04 §2.7)
- `pane.turn_ended {promptId, stopReason}`, `pane.queued {n, promptIds}`
- `pane.queue.paused {paneId, paused, reason}`,
  `pane.queue.resumed {paneId, paused}`. `pane.queue.steered` is introduced
  only with the proven steering method described by contract 04.
- `pane.permission_requested {requestId, generation, title, detail,
  options: [{optionId, kind, name}], expiresAt}`,
  `pane.permission_answered {requestId, result: selected|cancelled|expired,
  optionId?, by}`
- `pane.elicitation_requested {requestId, schema}`, `pane.elicitation_answered`
- `pane.config_changing {configId, value}`, `pane.config_changed
  {configId, value, options, sessionPreserved}`, `pane.config_failed
  {configId, value, reason}`
- `pane.harness_switch_started {switchId, from, to}`,
  `pane.harness_switch_done {switchId, from, to, sessionId, generation}`,
  `pane.harness_switch_failed {switchId, from, to, reason, restored: bool}`
- `pane.session_lost {reason, recovery: resumed|new_session|needs_login}`.
  Slice 4 adds `new_session_with_package` together with continuation packages.
- `pane.needs_login`, `pane.auth_finished {ok}`
- `loop.tick {paneId, round, of}`, `loop.stopped {paneId, reason}`
- `task.created`, `task.moved {actor, from, to, reason}`, `task.claimed
  {paneId, attempt}`, `task.released {reason, attempt}`, `task.commented`,
  `task.subitem_changed`, `task.blocked {kind, reason}`, `task.unblocked`,
  `task.phrase_changed {phrase}`, `task.updated {version}`,
  `task.attention {workspaceId, n}`
- `attention.changed {workspaceId, count, items}`
- `file.changed_on_disk`, `file.saved {by}`, `file.conflict`
- `lsp.diagnostics`, `editor.agent_cursor`, `editor.review_requested`
- `usage.changed {provider, state, primary?, secondary?, source, fetchedAt}`
- `pane.message {fromPaneId, toPaneId, text, state}`
- `device.pair_requested`, `device.paired`, `device.revoked`
- `engine.updating`, `engine.restarted {generation}`, `ptyd.reconnected
  {generation}`, `ptyd.lost`

## 6. Terminal byte stream (R18)
- `pane.stream.subscribe` → `{streamId, generation, firstRetainedOffset,
  nextOffset, binaryPaneRef (u32 used in the header)}`. `offset` = index of
  the next byte to read in the stream; a new process in the pane changes
  `generation` and resets the offset to zero.
- Type 2 frame: `[binaryPaneRef u32][generation u32][offset u64][bytes]`,
  bytes ≤ 64 KiB.
- `from: "tail"` → under the same lock that registers the live subscription,
  the engine captures `H = nextOffset` at that instant and calculates
  `start = max(firstRetainedOffset, H − 2 MiB)`, adjusted FORWARD to the next
  record boundary (at most 64 KiB ahead; the window is between 2 MiB − 64 KiB
  and 2 MiB). It sends `stream.replay_start {streamId, generation,
  fromOffset: start, toOffset: H, reset: true}`, delivers ALL of `[start, H)`
  in frames, `stream.replay_done {nextOffset: H}`, then live bytes starting
  at H. Barrier: no live byte before `replay_done`; no skip between replay
  end and live output. The client resets the terminal before applying
  replay with `reset: true`.
- Valid retained `from: {streamId, offset}` → `stream.replay_start
  {reset: false}` and deliver EVERYTHING from there, paginated in frames,
  without a gap and WITHOUT resetting the screen (a delta). Outside retention
  or different generation → `resync_required {firstRetainedOffset, generation}`
  and the client requests `tail`.
- Replay starts at a recording boundary (segment record); an escape sequence
  may be split between two records. This is expected: the client's terminal
  parser handles continuation. A `tail` starts at the beginning of the retained
  window and the client has already reset the screen, so reconstruction matches
  a terminal that read everything from that point onward.
- Gap in `ptyd` retention (contract 07): the engine writes a
  `gap {fromOffset, toOffset}` record in the segment, and replay delivers
  `stream.gap {fromOffset, toOffset}`; the client displays a “missing section”
  line instead of pretending continuity.
- Agent-opened terminals (contract 04 §5) arrive through `pane.update
  {kind: terminal_output, terminalId}`, not through this stream.

## 7. Errors
`error: {code, message, data: {kind, ...}}`. Stable `kind`:
`not_found`, `invalid_params`, `unauthorized`, `forbidden_role`,
`pane_busy`, `task_has_owner`, `transition_refused {reason}`,
`stale_version`, `idempotency_conflict`, `harness_unavailable`,
`needs_login`, `cursor_too_old`, `resync_required`, `slow_consumer`,
`protocol_version_unsupported`, `disk_full`, `ptyd_unavailable`.

## 8. Limits (R20) — single table, overrides every other mention
| Item | Limit |
|---|---|
| JSON body (type 1), any message including snapshot parts | ≤ 1 MiB |
| binary terminal body (type 2) | 16 B subheader + ≤ 64 KiB bytes |
| attachment body (type 3) | 16 B subheader + ≤ 256 KiB, chunked with `attachmentId` |
| `pane.transcript` per page | ≤ 200 messages or 512 KiB |
| comments within snapshot | 20 per task (remainder through `task.comments`) |
| retained event log | 10,000 events or 7 days |
| output queue per connection | 8 MiB (control before terminal); overflow = `slow_consumer` |
| connections per device | 8 |
| `tail` replay | 2 MiB |

**Attachment chunks.** The type 3 subheader is the 16 raw bytes of the UUID
written in `attachmentId`. Chunks for one attachment are contiguous and arrive
in order on the same connection. Their payloads are concatenated. The first
following JSON message that contains the matching `AttachmentRef` completes
the transfer and declares its MIME type, display name and total byte count.
The receiver accepts the reference only when the accumulated byte count equals
the declared decimal `bytes`; an unknown, duplicated, interleaved, oversized or
incomplete attachment fails closed. Unreferenced bytes are connection-scoped
and are discarded when the connection closes. When a response contains an
attachment, its type 3 chunks precede that JSON response on the wire.

**Multipart snapshot (without a `slow_consumer` cycle).** `state.snapshot`
does not return the snapshot in its response. The engine opens ONE read
transaction, sends the snapshot in `state.snapshot_part {snapshotId, index,
of, section, data}` notifications (each ≤ 1 MiB; sections: workspaces,
panes, tasks, loops, requests, attention, devices, capabilities, streams), and only
then responds with `{snapshotId, cursor, parts: of}`. Parts do NOT enter the
queue's 8 MiB budget: the engine queues the next part only after the previous
one was written to the socket (reader-paced). If the connection drops midway,
the client repeats the request; parts from an old `snapshotId` are discarded.

## 9. Name mapping table (R01)
| In contracts 01–07 | Canonical name (schema) |
|---|---|
| `de`, `para`, `motivo`, `ator` | `from`, `to`, `reason`, `actor` |
| `tipo` (blocking) | `kind` |
| `frase` | `phrase` |
| `pane.attention {n}` / `task.attention {n}` | `task.attention {workspaceId, n}` (per task) and `attention.changed` (inbox) |
| `pane.loop.*` | `loop.tick`, `loop.stopped` (with `paneId`) |
| pane states | single enum from contract 05 §1 |

## 10. Outside v1
More than one Mac in the same engine; cloud relay; HTTPS through `tailscale
serve`; per-workspace event subscriptions.

## Decisions applied to the two earlier questions
1. MCP = separate thin client; rules and persistence in the engine;
   `task.next` exists in the engine (`agent` role).
2. Terminal on the same socket, with a common type-and-size header.
