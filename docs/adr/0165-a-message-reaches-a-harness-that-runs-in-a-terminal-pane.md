# ADR 0165: A message reaches a harness that runs in a terminal pane, and agents talk to each other by name

- Status: proposed (phase 1: the design only; nothing here is built; the owner decided on 2026-11-14 that
  receiving and sending land before 0.1.0, and simplified the scope the same day, see "Scope")
- Date: 2026-11-14
- Builds on ADR 0148 and contract 17 (agents message and delegate to agents), contract 04 R17 (the
  attributed queued prompt and the full-access refusal between agent panes, #169), contract 08
  section 2 (roles), ADR 0069 (what a terminal pane's screen can be observed for), ADR 0161 (the
  protocol only grows), ADR 0079 (nothing blocks under the Core lock), ADR 0117 (a typed input is
  recorded before it is sent and not repeated blindly), ADR 0091 (no test backdoors) and ADR 0122 (a
  change of the supervisor counts for an update).

## Context

A harness runs in one of two ways. As an **agent** it is a pane that Soyeht opens over ACP, with the
Soyeht MCP, and `agent.message` between such panes works: it is attributed, limited to 6 a minute per
recipient, refused into a pane in full access (#169), and held until the recipient's queue can take
it. In a **terminal** the person types `claude`, `codex`, `opencode` or `cursor-agent` in a terminal
pane. The owner uses Claude Code that way every day, and both kinds must be able to receive and to
send.

### What the owner wants

The MCP of the **old Soyeht app** "works very well, except that sometimes it does not deliver a
message: it gets lost". The new Soyeht must do what the old one does, and not lose messages. The owner's
example: pane A runs Claude and pane B runs Codex (terminals or agent panes, possibly in different
workspaces). He tells them "work together: Claude makes the plan and sends it to Codex; Codex reviews and
returns OK or a review; at most two review rounds, on the third Claude starts implementing". They then
exchange messages **by name** ("message Livia", across workspaces) with **no human validating each
step**, over a long horizon, with named personalities per pane (one for the new Soyeht, one for films,
one for Lia). The new Soyeht adds agent panes (a switchable harness, accounts, local or OpenRouter
models) and web panes (HTML pages agents build that other agents edit), and messaging must reach all of
them.

### Scope (the owner's decisions of 2026-11-14)

- The harnesses are **Claude Code, Codex, OpenCode and Cursor**. Pi has no MCP and is left out of
  terminal messaging for now; it is a later candidate (its extensions could carry both directions).
- **No classification of connections by the process that made them** (an earlier draft made a program
  in a pane "never the person"). #169 stays exactly as it is for agent panes, and a **terminal
  recipient has no full-access distinction**: the engine neither looks for a harness's mode nor
  refuses a message because of it. What the owner does care about is not corrupting what the person is
  typing.
- **Windows works at launch**, with the same pieces as the other systems.
- Soyeht **connects the harnesses** with one consent screen (first run and Preferences), as the
  Pencil design app does, instead of per-launch flags.
- Native channels other than Claude's hooks come after launch.

### What an end-to-end run found (origin/main 2dc4d381, 2026-11-14)

1. **A terminal pane is invisible to messaging.** `agent.list` lists only `PaneMode::Agent` panes of
   the sender's workspace, and `agent.message` to a terminal answers "Target agent pane not found"
   (`agents/methods.rs`, `agent_peer_method`). Nothing is typed. Typing is mechanically possible
   (`pane.input`: bracketed paste, a short pause, `\r`), but a manual test merged the message into
   half-typed text in the harness's input line.
2. **A terminal harness cannot send.** `soyeht-mcp` needs a binding that only an ACP session gets
   (`agents/mcp.rs`; `dispatch.rs` `agent_identity` looks in `self.agents` only).
3. **Turn boundaries are known only sometimes.** `pane.create` with `mode=terminal` and a harness adds
   `--settings` hooks for Claude (`terminal_harness.rs`, `agents/hooks.rs`); they exist only when
   Soyeht started the harness, are installed for the experiment's labels and are a no-op without a
   capture.
4. **`native_text` forges its own route** (`agents/peer.rs`): `source.name` goes raw into the first
   line, and the reader splits that line on ` (` and the route text, so a pane named with a
   parenthesis and the route text can make a message look like it came from another pane. Fixed first
   and on its own (slice 0b), for agent recipients too.
5. **Names do not cross workspaces.** `agent.list` and `agent.message` stop at the sender's workspace,
   and a pane is addressed by id only.

The screen-state recorder (#221, `docs/research/terminal-screen-recorder.md`) was built to find out
when a terminal's input line is safe to type into. **No recording exists yet** (the run worktree holds
none, and the recorder's default folder is absent on this machine), so every threshold below is a
starting value with its reason, and "What the data must show" says what to collect and the bar.

### Known limitation, not changed here

A program of the same user, including one started in a terminal pane, can connect to the engine's
local endpoint as the person: `soyeht pane prompt`, or a raw `pane.prompt` or `pane.input`, reaches
any pane, an agent in full access included, with no attribution, and #169 (which stops an agent's
`message_agent`) does not see it. This is the boundary contract 08 already states ("isolation is per
OS user, not per process") and ADRs 0128, 0162 and 0164 repeat. It is listed in SECURITY.md and in
contract 08 (slice 0b), and revisited after launch.

## How the old app does it, why it lost messages, and what we do instead

This section is read from the old app's source and its own QA records, read-only (the 2026-09-16
`agent-messaging-delivery` checkout, `scripts/soyeht_mcp_tools_messaging.py`, the Mac app's
automation router and pane delivery coordinator, `docs/agent-messaging-delivery-hardening.md` and the
`QA/runs` reports). Nothing was run, and no tool that drives the owner's live old app was used.

### How the old app works

- **Transport.** The MCP server is a Python script. Each call is a JSON **request file** written to a
  spool folder in the app's data directory; the Mac app watches the folder, does the work and writes
  a response file. There is no connection, so the app learns that an agent is "there" from a
  **heartbeat** request that each MCP server repeats.
- **Who is calling.** The pane's environment carries `SOYEHT_CONVERSATION_ID`, `SOYEHT_HANDLE`,
  `SOYEHT_LAUNCH_NONCE` and the spool folder. The MCP server reads its own environment, then the
  environment of up to four ancestors (through `ps`), then its controlling terminal, and the app
  checks the process identity (pid and start time). The code notes that Codex does not inherit the
  folder variable, so such a server is found through the window id and the terminal.
- **Names.** Every pane has a lower-case handle, unique among all panes (the store compares the
  normalised form); `[name]` is the display form, and prose uses it so a name copied into a commit
  mentions nobody. `list_agents` is a **global directory** of all workspaces, grouped, with
  `canReceiveMessage` per pane and the reason when false (`mcp_not_connected`, `agent_not_running`,
  `not_live`); `message_agent` takes a handle or a conversation id and answers per target
  (`delivered`, `queued`, `blocked`, `target_not_found`, and the three reasons above). The tool
  descriptions teach the model the rule: for a communication request with names, call `list_agents`
  first, route matches to `message_agent`, never spawn an internal subagent as a substitute, report
  unmatched names instead of creating panes, and never claim a reply until a real one arrives.
- **Who may send to whom.** A per-pane and per-workspace policy: incoming and outgoing enabled,
  cross-workspace allowed, blocked pane and workspace lists; denial always wins; the default is open.
  An agent can restrict its own pane (`set_agent_communication_policy`), never open it. A workspace may
  also carry a role template per pane (planner, executor, reviewer, aggregator, or custom) and an
  **orchestration graph** (presets: council, planner-executor-reviewer, executor-reviewer-loop) that
  closes messaging to the graph's edges and delivers each pane's role instructions as messages from a
  control-plane sender. There is no round counter anywhere: "two review rounds" is an instruction to
  the agents.
- **Storage.** A message is stored first, in the **recipient's durable inbox** (inside the workspace
  snapshot, with bounds of 1000 pending and 64 KiB a body), with separate read and acknowledgement
  marks (`list_agent_messages`, `ack_agent_messages`). The "semantic inbox" channel, which would wake a
  harness to read it, is declared but **never enabled** in the code (`canWakeAndReadSemanticInbox` is
  false); an explicit inbox request is only stored, and the receipt says it could not wake anyone.
- **Delivery into a terminal.** So every message goes by the compatibility path: the Mac app **types the
  whole message into the recipient's terminal**, as one bracketed paste and Return, in an envelope:
  `Sent via Soyeht. From: [sender] (conversationID: ...). To: [recipient] (...). Soyeht-Delivery-ID: <uuid>.`
  then the recipient's role text if it has one, `Reply via Soyeht MCP <server>.message_agent to
  conversationIDs=["<sender id>"], lineEnding=enter. Request: <body>`. The recipient harness reads it as
  an ordinary prompt and acts on it; it answers with `message_agent`. That is the behaviour the owner
  likes: the recipient gets the full text and acts, no human in between.
- **Not typing over the person.** The app keeps a **draft gate**: a state machine that counts the
  keystrokes (and escape sequences, Kitty keyboard packets, mouse reports, paste markers) the person
  sends to the pane, to know whether the input line holds an unfinished draft. A message waits for a
  clear gate, a 0.75 s grace after the last human input, a provider state that is not `blocked`, and a
  ready transport. A badge on the pane header counts queued, unread and failed messages, and the
  person can confirm the field is empty to release them.
- **Knowing it arrived.** The delivery is claimed in the store **before the first byte** is written (at
  most once), and the app then waits up to 15 s for the harness's own `UserPromptSubmit` hook to report
  the delivery id. Later versions also make the receiving hook compare the exact bytes (a SHA-256
  expectation written before the paste) and **block a clipped prompt before the model sees it**.
  Where no hook exists the receipt is `transport_only`.

### A real round trip (2026-11-14)

A round trip run through the old app's MCP at the owner's request, with no change to it, showed the
parts above end to end and the failure in the next section. `identify_agent` resolved the sender's
pane by its terminal alone; `list_agents` returned the target with `canReceiveMessage: true` and a
ready `messageTarget`; `message_agent` answered `status: queued_until_human_input_is_clear`,
`channel: deferredTerminal`, `writesToPTY: true` and a durable message id. The recipient's harness
received, as a user turn, typed and submitted:

`Sent via Soyeht. From: [livia] (conversationID: 313C9D27-...). To: [novo soyeht] (conversationID: ACE407A9-...). Soyeht-Delivery-ID: F8C15691-.... Reply via Soyeht MCP soyeht.message_agent to conversationIDs=["313C9D27-..."], lineEnding=enter. Request: ping`

That header is some 70 tokens, most of it three UUIDs, for a message of one word. The owner wants the
same mutual identification, much shorter.

### Why it sometimes lost a message

Read from the code, the tests and the history of that branch (200 commits in two days of hardening):

1. **A truncated paste, reported as delivered.** Three real production messages arrived with their
   first 1022 bytes missing. The mechanism, reproduced on Claude Code 2.1.268 and 2.1.273: a message
   written as bulk input **without bracketed paste framing** is clipped by the harness's input
   parser; with framing it is intact. The app wrote unframed when it believed paste mode was off, and
   it could believe so wrongly: a freshly attached terminal view receives the engine's default tail
   of output, which omits the mode announcement the application made long before, so the view's
   parser had lost "paste mode on". The first receipt only matched the delivery id, so the clipped
   text, id and all, counted as delivered.
2. **Receipts that proved nothing.** `transport_only` meant only that the bytes reached the local
   transport; a TUI can swallow the Return. A timeout then left the message "uncertain".
3. **A message held by a false "the person has a draft", with no way out.** The old app decides
   "human input is clear" from **bookkeeping of the keys the person sent** (`AgentMessageDraftGate`,
   fed by every byte the terminal view writes to the pane): printable characters add to a count,
   Backspace subtracts one, Enter or Ctrl-C resets it. Any key it cannot model sets "draft state
   unknown": Tab, Up and Down (history recall), Ctrl-U and every other control character, Escape
   chords (Alt-Backspace and the other Alt keys), modified or unsupported keys, a paste over a draft,
   invalid UTF-8, a slash command picker (its latch survives Return). Unknown, and a non-zero count, are
   **cleared only by Enter or Ctrl-C**, by an authenticated prompt-submit hook, by a person's
   confirmation of the badge, or (for a few causes) by the harness's first MCP registration. It never
   looks at what the screen shows. So a person who presses Tab, Up or Esc at an **empty** input line
   (or types a character and deletes the word with Alt-Backspace, or answers a menu with a keypress)
   leaves the pane "in draft" while the line is empty. That is exactly what the live round trip
   showed: the reply was accepted at once, **the input line was empty**, the message waited, and it
   arrived only right after the person pressed Enter on something in that pane. A second pane showed
   the same ("waiting for you to clear its text field"). Other holds join it: provider-state
   evidence from hooks that must be fresh and verified, a 0.75 s grace after any human byte, a
   transport-ready flag. And the hold has **no visible meaning to the sender**: `message_agent` returns
   a success-shaped answer (`queued_until_human_input_is_clear`), the sender's agent told its person it
   had replied while nothing had arrived, and the only places the wait shows are a badge on the
   recipient's header and a confirmation dialog there. A 60-second queue deadline added to bound it
   then failed legitimate messages whenever a person kept a draft for a minute, and was removed again
   after review. The recipient had no other channel: the "semantic inbox" that could have reached the
   harness without its input line is never enabled.
4. **Messages thrown away on a presence change.** A message was delivered only to a pane with a
   connected MCP client, and if the client disappeared during the grace period the undelivered
   message was **removed** rather than kept.
5. **At most once, with no recovery.** The claim before the first byte means a crash, a transport
   failure or a lost acknowledgement leaves `uncertain_not_replayed` for good; the design never
   retries. A message that may or may not have arrived is, in practice, lost until a person acts.
6. **A spool that drowned the app.** Abandoned heartbeats and queries piled up while the app was
   closed or busy (the QA found 12,130 stale requests, and an earlier 49,669 files); each completion
   rescanned the whole folder on the main thread, the app stalled, requests timed out, and the sender
   could not tell what had happened. Fixed late, by expiring the queries and scanning off the main
   thread, but it is the cost of a file transport.
7. **A pane with no way to be found.** A harness that does not inherit the pane's environment (the
   code names Codex) is matched by terminal or by window; a wrong window or the Dev against the release
   folder routes a message to the wrong app or to nothing.
8. **Only while the app runs.** Deadlines and delivery are evaluated by a running pane's coordinator;
   with the app closed nothing happens, and the in-memory queue of a pane is dropped when its
   transport is replaced (the durable inbox survives, the delivery attempt does not).
9. **Rejections of intact messages.** Claude strips a trailing space the app added as padding, so the
   byte-exact hook rejected a correct message until the padding was kept out of the hash.
10. **Names.** A handle is resolved by the first pane whose normalised handle matches, and there is
    no "which Livia" answer (handles are unique in the store, so this is a smaller risk than the rest).
11. **"Queued" read as "delivered".** The sender is never told when a queued message is delivered,
    stuck or ended, and the tool description does not say that queued is not delivered.

### What this ADR does instead

| Old behaviour or loss | Here |
|---|---|
| Types the whole envelope into the terminal, and the recipient acts on it | **Kept**: the typed channel delivers the **whole attributed message**, not a pointer to it (no doorbell). |
| Paste clipped when paste mode was believed off | The engine's own terminal model sees every byte of the pane from its start and tracks paste mode itself. It **never types a message unframed**: unknown or off means "not now", never "raw". Bodies are tested at 1022, 1024, 4096 and 60000 bytes on each harness. |
| `delivered` on a transport receipt | States are honest and separate: `typed`, then `confirmed` only on evidence (a hook that verified the exact bytes, or the message id and the submission visible on the rendered screen); else `unconfirmed`. |
| "Human input is clear" decided from key bookkeeping that clears only on Enter | The **safe-input-line rule** from the **rendered screen** plus the recency of the person's input (below); no count of keys, nothing that waits for Enter. It is stateless, re-evaluated from the screen each time, so Tab, arrows, Esc, Alt keys and menus leave nothing behind. A **watchdog** treats "blocked for a minute while the person did nothing" as a suspected false positive and escalates it. |
| A hold with no visible state, no timeout notice to the sender, no other channel | The strip says what is waiting and why ("1 message waiting: the input line has text"); the sender's status call and a note to the sender say the same, truthfully (`queued` is not `delivered` is not read); and two ways that need no input line (Claude's hook at a turn boundary; a waiting count on every tool result of `soyeht-mcp`), then Deliver now. |
| A 70-token header of three UUIDs | A **compact header** of names and a short id, about a fifth of the size (below). |
| Expiry or removal of a waiting message | **No expiry in minutes.** A message waits for a safe moment for up to 24 hours (`terminalMessageTtlSec`), is kept across a presence change, a restart and a harness change, and when it ends, both sides are told. |
| At most once, uncertain forever | **At least once, idempotent by message id**: an attempt that did not land (a clipped prompt blocked by the hook, no sign on the screen, a swallowed Enter) is retried, up to 3 attempts, each only when the safe rule holds; the header says `repeat`, which the model reads as "ignore it if you already handled it". A duplicate is possible and visible; a silent loss is not. |
| File spool, heartbeats | A **long-lived connection** from `soyeht-mcp` to the engine's local socket: presence is the open connection, there is no spool and no polling, and a request has an answer or an error. |
| Identity by environment, ancestors and terminal | A **pane-scoped binding** in the pane's environment, the same environment-first idea, with a measured fallback per harness (gating spike) and the controlling terminal as the last resort. |
| Delivery only while the app runs | The **engine** owns the durable inbox and the delivery; the app is a client. |
| Names: first match | **Names across workspaces**, matched without case or accents; two matches is an `ambiguous` error that lists the candidates, never a guess. |
| Roles and orchestration graphs | **After launch** (below); the personality of a pane is plain text the person gives its harness. |
| Spool backlogs hiding failures | Every message has an observable state, a strip on the pane, an event, and a status call for the sender. |

## Decision

1. **Terminal harnesses are first-class messaging endpoints**: listed by name with agent panes, in every
   workspace, addressed by name or id, with the same attribution and the same limits.
2. **The sender** is identified by a pane-scoped binding in the pane's environment, read by
   `soyeht-mcp` (a stdio server the harness starts), over a long-lived connection to the engine.
3. **The recipient** has a durable inbox in the engine. The **whole message**, under a short header of names
   and a short id, is typed into the harness as one bracketed paste and Enter, when and only when the rendered input line is provably empty and
   quiet; for Claude a hook delivers it at a turn boundary with no typing. Delivery is confirmed by
   evidence, retried at least once idempotently, and observable: a wait always shows its reason to the
   person and to the sender, and has a way out.
4. **Messaging is autonomous**: no person approves a message; caps stop a runaway loop, and a person
   can close any pane or workspace to messages with a policy.
5. **Soyeht connects the harnesses** it finds installed, on the person's consent, writing each one's
   user-level MCP configuration; reversible.
6. **Additive protocol** (ADR 0161).

### 1. The sender

#### The binding

For every terminal pane (a plain shell included, because the person types `claude` whenever they like)
the engine makes a binding when the pane's process starts and puts it in the process's environment
(`Spawn.env`, which the supervisor already carries; on Windows, the environment block of the pane's
process), with the pane's id. It keeps the binding's SHA-256 only. The binding **ends when the pane's
process is replaced** (a restart, a harness switch): it is tied to the pane's instance key and
generation, and a new process gets a new one. A stale binding gets `unauthorized`.

A connection that presents it is granted the `agent` role for the messaging methods (list, message,
status, inbox, policy of its own pane), for its own pane only. Task tools stay with the ACP agents in
this phase. The sender's name and pane id travel in the same `AgentMessageSource`, with
`kind: terminal`.

`soyeht-mcp` gains a mode with no pane arguments: it reads the binding from the environment and keeps
**one long-lived connection** to the engine, which is the presence of the pane's harness. **Started
outside a Soyeht pane (no binding) it does not fail**: it answers the MCP handshake with no tools and
an instruction that says it is not inside a Soyeht pane, so a harness the person runs elsewhere shows
no error. The command line gets `soyeht message TARGET TEXT`, `soyeht inbox read ID` and
`soyeht message status ID`, which read the same variable.

**Plainly: any process that has this environment can act for the pane.** That is every process the
pane runs, including the commands a harness's own shell tool starts, and a same-user process of
another pane that reads this pane's environment (`ps eww`, `/proc/PID/environ`, and the Windows
equivalent). This is the limit of the existing ACP binding too (contract 08). The sender's limits
(attribution, the caps below) bound the harm: the person always sees who a message claims to come from.

#### Does the harness hand its environment to its MCP server? (gating spike, slice 0)

The old app hit this: Codex did not hand its environment to its MCP server. Three of the four run a
shared background process that parents their tools: Codex's local app-server (`codex app-server`;
the review found it at 0.169.0 while the CLI is 0.164.0), OpenCode's background service
(`opencode service`; `--standalone` runs a private one; a running `opencode` has parent 1 on this
machine) and Cursor's `persist` mode. An MCP server such a daemon starts has the daemon's environment,
not the pane's. Slice 0 starts each of the four in a pane with a **probe stdio MCP server** that
records its environment variable, its parent chain and its controlling terminal, and does the same for
each harness's shell tool (`soyeht message` needs the variable too). Until it runs, this table is the
plan, not a fact:

| | Expected | If the variable does not arrive |
|---|---|---|
| Claude Code | MCP servers are children of the harness: the binding arrives | the entry's `env` carries a `${VAR}` reference (its configuration expands them) |
| Codex | maybe not (shared app-server) | the entry names the variable to forward (`env_vars`); else, for a Codex Soyeht starts, `-c` overrides with the binding in the per-session MCP entry; else Codex receives and cannot send until started from the pane's launcher |
| OpenCode | maybe not (background service) | the entry's `environment` with a reference; else, for a launch by Soyeht, `--standalone`; else it receives and cannot send |
| Cursor | maybe not (`persist`) | the entry's `${env:VAR}` reference; else a launch by Soyeht with the flag that keeps it attached; else it receives and cannot send |

Fallbacks after that, in the order the old app used them: the **environment of the ancestors** (read
through `soyeht-platform`; it helps when a launcher sits between the harness and the server), then
**which pane owns the server's controlling terminal** (the engine knows each pane's pty; it works only
when the server has the pane's terminal, which a daemon's child does not). **Receiving works for all
four whatever the spike finds** (the typed channel needs no identity from the harness); **sending
works for each harness that gets the variable or is attached to its pane**; one that does neither
answers "this harness is not attached to a Soyeht pane; start it from the pane's launcher", and the
connection screen says so. The spike gates every promise to send.

### 2. Names, workspaces, policy and caps

- **Names.** A pane's name is its address. `list_agents` returns every messageable pane **of every
  workspace**, grouped by workspace with the sender's own first, each with `name`, `paneId`,
  `workspace`, `kind` (`agent` or `terminal`), the harness, whether it can receive now and the reason
  if not, and whether it can reply (a connected `soyeht-mcp`). `message_agent` takes a `targetName`
  (compared without case, accents or surrounding space) or the pane id (unambiguous, and what the
  envelope's reply route uses). **Two matches answer `invalid_params` with the candidates** (workspace
  and id), never the first. The tool descriptions teach the model the old app's rule: for a
  communication request with names, call `list_agents` first; route matches to `message_agent`; never
  stand in an internal subagent for a matched pane; report an unmatched name instead of creating a
  pane; never claim a reply that has not arrived.
- **Across workspaces.** The same-workspace restriction of `agent.list` and `agent.message` goes. What
  keeps messaging deliberate is the policy below, open by default as in the old app because the owner
  wants agents to work together unattended.
- **Policy.** Per pane and per workspace: incoming and outgoing on or off, cross-workspace allowed or
  not, blocked panes and workspaces. The four layers (source pane, source workspace, recipient pane,
  recipient workspace) are intersected: **a denial anywhere wins**. A person sets any of them in the app;
  an agent can only **restrict its own pane**. A refusal names the denial (`forbidden_role`). #169 is
  evaluated after it and is unchanged.
- **Web panes.** A web pane has no input line and no harness. It is listed as `kind: "web"`, not
  messageable, so an agent can find "the dashboard page" by name and work on it with the web tools. A
  message addressed to a web pane is open question 1.
- **Caps** (new, for every message with a terminal at either end; autonomy is the point, so they are
  safety nets and not rules of the workflow): the existing 6 a minute per sender and recipient; **12 a
  minute per recipient in all**; **one undelivered message per sender and recipient pair** (a second
  to the same recipient is refused with `pane_busy` until the first is delivered, ended or dismissed);
  and a **hop counter**: a message a pane sends within 120 s of receiving one carries its hop plus
  one. At hop 24 the engine **pauses the chain and asks the person** in the strip ("These agents have
  exchanged 24 messages. Continue or stop"); it never drops the message. The owner's "two review
  rounds" is an instruction to the agents and is not enforced here.

### 3. The recipient

#### Detecting a terminal harness

A terminal pane has a harness when the **foreground program of its terminal** is one. The supervisor
reports it (one request, `Foreground`, for a list of instances):

- **macOS and Linux**: the process group of the pty from `tcgetpgrp`, then its leader's executable
  path, `argv[0]` and arguments.
- **Windows**: a pseudo console has no foreground group. The supervisor holds a job object per pane;
  it lists the job's processes (`QueryInformationJobObject`), builds their tree from the parent ids and
  creation times (a recycled id breaks the chain), ignores the console host, and takes the
  **shallowest process whose image or script is a known harness**, reached from the shell only
  through shells and launchers. A harness's own children (its MCP servers, its tools) are deeper and
  never matched. It is a heuristic, so on Windows the rendered input line must also match the
  harness's profile before anything is typed. A harness inside WSL, `tmux`, `screen`, `ssh` or a
  container is not seen (its processes are not the pane's) and the pane is a plain terminal.

Matching reads the **executable path, `argv[0]` and the script argument**: `cursor-agent` is a shell
script that runs `node` under its own name with a script file, and `claude` is a symlink to a
versioned binary. A table in the engine maps the four harnesses (and `agent`, the short name of
Cursor's program) to a harness id and a profile; slice 2 tests it against the real binaries on each
system, not fixtures. The engine asks on a slow tick (2 s, for panes of a workspace with a waiting
message or a sender that listed) and on demand, never in a loop that costs a pane that runs a shell.

A terminal pane is **listed** when its foreground is a known harness. It can receive whether or not its
`soyeht-mcp` is connected (the old app refused such a pane and lost the message; here the message is
delivered and the envelope's reply route also names the command-line fallback `soyeht message`).
`Pane` carries an optional `terminalHarness {id, name, version}` for the apps' header.

#### The inbox and its states

Every message is stored first, in the recipient's **durable inbox** in the engine (`soyeht-store`, one
table), whatever the recipient's kind; the existing ACP prompt queue keeps carrying agent panes, and
the inbox carries terminals. `agent.message` returns at once with `queued: true`, a `messageId` and
`delivery`. An inbox holds at most `terminalMessagesPending` (50), in order, one at a time, and each
waits up to `terminalMessageTtlSec` (24 hours), kept across a restart, a presence change and a harness
change. The body limit is the agents' (`agentMessageBytes`, 60000).

States: **queued** → **typing** (an attempt is being written) → **typed** (the bytes were accepted
and Enter sent) → **confirmed** (evidence below). Other ends: **needs_you** (attempts exhausted, or a
step could not be verified), **expired**, **dismissed**. The sender sees the state through a status
call and is told of every end that is not `confirmed`; the person sees it in the strip.

What the sender is told never overstates: **queued** (stored, not delivered), **delivered** (`typed` and
`confirmed`: the harness took the prompt) and **read**, only where there is evidence the model saw it
(Claude's hook verified the exact bytes, or the recipient answered with `inReplyTo` the message id).

- **An attempt is claimed before it is written**, with a short **lease** (about 10 s, and the
  attempt's number): the claim and the supervisor's input are recorded together (ADR 0117), so two
  channels, or a restart, never write the same attempt twice. A claim whose lease expires with no
  outcome becomes `needs_you`, not `queued`: the engine does not guess whether the bytes landed.
  That is the one case where a person decides.
- **Retry is evidence-based and bounded.** After `typed`, the engine waits for confirmation. If the
  evidence says the attempt did not land (the receiving hook rejected a clipped prompt, or after
  `terminalConfirmMs` the message id is nowhere on the screen and the harness is idle with an empty
  input line), the next attempt is made, at most 3 in all, each when the safe rule holds again, with
  the same message id and `repeat` in the header (read as "ignore it if you already handled it"). If
  the evidence is that it did land, nothing is repeated.
- **Reading by the harness's own tools** (below) is a second way to take a message, so a harness that
  cannot be typed into still gets it at the next hook or tool call. A claim by one way makes the
  other skip the message.

#### What is typed: the whole message, with a short header

The old envelope's job (who sent it, to whom, how to reply, which delivery) in a fraction of the
tokens. Routing is by **name, resolved by the engine**; the text carries no UUID. One function builds
it for every recipient kind (`native_text` in `agents/peer.rs`):

```
[livia → novo soyeht · m:F8C1]
<body, newlines kept>
```

- `livia` and `novo soyeht` are the sender's and the recipient's names. When the two are in
  different workspaces, or the name is not unique, the sender carries its workspace
  (`livia@filmes`). The engine resolves any name it receives, so a reply needs only the name.
- `m:F8C1` is a short **engine-issued message id** (four or five characters, unique among the
  recipient's open messages; `m:F8C1 · repeat` marks a retry of the same message, which the model
  reads as "ignore if you already handled it"). The sender's tool, the status call and the strip use
  it; `message_agent` accepts it as `inReplyTo`, and a reply that carries it counts as evidence that
  the message was read.
- **How to reply is not repeated in every message.** The instructions the `soyeht-mcp` server gives
  its harness (and the descriptions of its tools) say once: "a message starting `[from → you · m:ID]`
  is from another Soyeht pane; answer with `message_agent` to that name". A harness that has no
  connected `soyeht-mcp` gets one extra line in its first message: `(reply: soyeht message livia "...")`.
- The old header was about 70 tokens, mostly three UUIDs; this one is about a fifth of that, and it
  grows only with the names.
- The first character is `[`, which none of the four harnesses reads as a command, a shell escape, a
  file or a note (slice 0 checks it on each: `/`, `!`, `@` and `#` are the characters that would
  matter). Names are **one line of at most 40 characters without `[`, `]`, `(`, `)`, `·`, `→` or
  `@`**, whatever the pane was named, so a name cannot forge the header.

#### Text from another agent is data

On **every channel and for every recipient kind** the engine reduces any text it passes on (the body,
the name, the preview) to printable characters, newline and tab: controls (so no escape sequence, and
no end-of-paste marker), the 8-bit controls, and **format, private-use and unassigned code points**
(bidirectional and zero-width characters, the tag block U+E0000 to U+E007F) are removed, and `\r`
becomes a newline. This is the old planner's rule made total: the engine, not the sender, builds the
bytes, and it adds the paste markers.

#### How it is typed: one paste, framed, verified

The typed channel writes the envelope as **one bracketed paste followed by Enter**, only when all of
the following hold, in this order. They are about what the engine can observe; none counts or
interprets a keystroke.

1. **Recipient.** The foreground program is still the harness, the pane has a process, no restart or
   switch is pending, and the harness's version is in the range its profile was verified for (below).
2. **The person is not at the keyboard.** At least `terminalQuietMs` since the last `pane.input` the
   engine accepted for this pane, from any client (it records only the time).
3. **The screen has settled.** The pane's rendered screen has not changed for `terminalSettleMs`. A
   harness at work redraws a spinner or a clock every second, so a quiet screen also marks an idle
   turn; the value must exceed twice the largest redraw gap measured in a real turn. Where the harness
   reports its turn (Claude's hooks), the turn must also be idle.
4. **The input line is the harness's, and it is empty.** This **replaces the old keystroke-counting
   draft gate**. The engine keeps a terminal parser for the pane (the model of `terminal_projection`,
   fed off the Core lock through a bounded queue as the experiment's capture is; an overflow marks the
   screen unknown until it is rebuilt from the pane's retained output). The harness's **profile** says
   where the input line is (from the cursor and the box or rule the harness draws), what an empty one
   looks like (the prompt glyph and nothing the person wrote; a dimmed placeholder is not text), and
   which marks mean the line is not for text: a permission dialog or a menu, a history search, an
   editor mode line that says it is not inserting (Claude's vim mode shows `NORMAL`), and a screen
   (main or alternate) the harness is not drawing on (Codex draws on the alternate screen by default
   and has an option to turn it off, so the value is the profile's and the data's). The profile is
   **data**: one generic procedure reads four different TUIs, so there is no code per editor and no
   rule for what an editor's keys do (a Vim `dd` is irrelevant when the line is empty). When the
   profile cannot say "empty", the answer is "not safe", never "probably". Because it reads the
   screen, it cannot be wedged by a key it does not understand: whatever the person did, the line is
   either empty or it is not. On Windows this rule also confirms the harness is the one holding the
   console.
5. **Paste is framed, and the engine knows it.** The terminal parser has seen every byte of the pane
   since its process started and tracks bracketed paste mode itself; it is written to the store so an
   engine restart does not lose it, and rebuilt only from retained output that is complete since the
   last reset. Mode on: the message is pasted framed. Mode unknown or off: **nothing is typed**, the
   message waits (`waiting_mode` in the strip, Deliver now for the person); it is never sent raw. This
   is the lesson of the 1022-byte clipping. ConPTY's passthrough of the markers is unproven, so the
   Windows slice tests it (no raw fallback for a message; the person's Deliver now remains).

The write is one supervisor input with a **guard checked at the instant of the write** (the pane's
foreground is still the one decided on; on Windows, that the harness's process is still in the job).
If the harness has just exited, the paste would land in a shell; the guard refuses it. Enter is a
separate write sent only after the echo check: within `terminalEchoMs` the rendered input line holds
the envelope's first line (or the harness's collapsed form of a paste; the profile knows it). Anything
else (the person typed in between, the foreground changed) sends **no Enter**: the attempt becomes
`needs_you` and the person is told. This limits the cost of the race between the last check and the
write to one visible paste with no Enter.

#### Confirmation

- **Claude Code**: the hooks. Before the paste, the engine records an expectation (the digest of the
  exact bytes and the message id) with the claim. Claude's `UserPromptSubmit` hook, which the connect
  step installs and which is Claude's child (so it has the binding), compares the prompt it receives with
  the expectation and **blocks a mismatch before the model sees it**, reporting it; a match reports
  `confirmed` with the exact bytes. The padding that broke the old guard does not exist here: nothing is
  added to the text.
- **Codex, OpenCode, Cursor**: no hook at launch. The evidence is on the screen: after Enter the
  harness shows the submitted prompt (the message id `m:...` is on the screen, or the profile's collapsed
  form) and either works or returns to an empty line. That is `confirmed`; nothing visible after
  `terminalConfirmMs` is `unconfirmed`, and the retry rule above applies. A harness's own reply
  through `message_agent` that carries `inReplyTo`, or a `soyeht_inbox` read of the message, is also
  confirmation.
- Codex's hooks (they need the person's trust, and a new trust at every change), OpenCode's plugins and
  Cursor's hooks can verify the exact bytes after launch, each proved on its real binary first.

#### Claude's native channel (at launch)

For a Claude whose connect step installed the hooks, a message can also be taken **with no typing**:
at the end of a turn (`Stop`) the hook pulls the pane's pending messages and prints them as the reason
to continue, so the message starts the next turn with the person's half-typed text untouched, and at
`UserPromptSubmit` as added context. A Stop hook that is itself the continuation of a turn it began
(`stop_hook_active` in its input) **delivers nothing**, so a message never starts a chain of its own.
The hook reads the binding from its environment, prints nothing outside a Soyeht pane, when no engine
answers within 1.5 s, or when the binary is gone. The pull and the acknowledgement are **new methods**
granted to a binding for its own pane: a pull claims the message with a lease and returns its body, an
ack ends it (`confirmed`); an expired lease is `needs_you`. The inbox path never starts or feeds the
experiment's recording. These behaviours are documented for the version tested, the e2e test pins them,
and a version that stops honouring them closes the channel and the typed channel takes over.

#### Profiles are tied to the harness version

A profile lists the versions it was verified for. The engine reads the version once with
`<harness> --version` through `soyeht_process::run` (a deadline, never under the Core lock), cached by
the executable's path and modification time, and shows it in `terminalHarness`. Outside the range the
automatic typing is **off** (rule 1) and the message waits, visibly, with **Deliver now** for the
person; a profile for a new version is a data change with its vectors, not an engine release.

#### When it does not become safe: nothing is held in silence

A message waits at most 24 hours, kept, and its state always has a **reason** (`typing`, `line_has_text`,
`busy`, `unknown_screen`, `waiting_mode`, `version`, `not_a_harness`). What the old app lacked is
everything around the wait:

- **The person sees it at once.** The strip says "1 message from livia is waiting: your input line
  has text" (or the reason), with Deliver now and Dismiss; the same count rides the snapshot, so any
  client shows it.
- **The sender is told the truth, early.** `message_agent` answers `queued`, and says in words that
  this is **not delivered**: "queued for livia; not delivered yet; ask `message status m:F8C1`". Its
  tool description says that queued is not delivered and delivered is not read, and that the
  agent must not tell its person that the recipient has the message until the state is `confirmed`
  or a real reply arrives. If a message is still waiting after `terminalEscalateSec` (60 s), the engine
  writes the sender a note in its own pane: "m:F8C1 to livia is still waiting: the input line has
  text". Every end that is not `confirmed` (expired, dismissed, `needs_you`, attempts exhausted) is told
  the same way, with its reason. That note is engine text, safe even for a sender in full access.
- **A watchdog catches a false positive.** The old failure was a hold that the person did not cause.
  When a message has waited `terminalEscalateSec` while the person's last input is older than that and
  the profile still says "not safe", the engine marks it **suspected false positive**: it raises the
  person's attention (the pane's attention mark and, where the system allows, a notification), keeps a
  bounded diagnostic of the rows around the cursor (kinds and positions only, no text) in the log,
  and offers Deliver now.
- **Channels that do not need the input line.** Claude's hooks (above) take the message at the next
  turn boundary. For every harness, **every tool result of `soyeht-mcp` carries a waiting count**
  ("2 messages waiting; read with `soyeht_inbox`"), so a harness that calls any Soyeht tool learns of
  them whatever its input line holds (the pull is the same two-phase claim). The typed channel is
  what reaches a harness that is idle and calls nothing.
- **Deliver now** is the person's own action: it pastes the message at once **without Enter**, and the
  person looks at it and presses Enter. It is the floor under every case above.

#### What the person sees

A **strip under the pane's header**: "Message from NAME (harness)", the time, its state, the first
`terminalMessagePreviewChars` (160) characters, with Deliver now, Dismiss and, at a paused chain,
Continue and Stop. It is flat, in palette roles and shared tokens, like every pane interior. It is
driven by a new event with the message's state and by the snapshot (open messages belong to the
pane), so a client that reconnects shows it. In the harness the person sees the typed message, and the
harness's own transcript.

### 4. Connecting the harnesses

Like the Pencil design app: **on first run and in Preferences, Soyeht looks for the harnesses that are
installed and shows one consent screen.** It lists each harness found (name, version, where it was
found), the **exact file each connection writes** with a "Show change" view of the entry, and one
switch per harness. Connect writes; Disconnect removes. Nothing is written before the person agrees,
and no per-launch flag is needed once a harness is connected.

- **Detection** looks in the `PATH` and in the places each harness installs to (per system: the user's
  `~/.local/bin`, Homebrew and npm global folders on macOS and Linux; `%LOCALAPPDATA%`,
  `%APPDATA%\npm` and `%USERPROFILE%\.local\bin` on Windows), and reads the version with
  `soyeht_process::run`. A harness that is not installed is not offered.
- **What is written** is one MCP server entry for `soyeht-mcp` (absolute path; its `--data-dir` pins the
  profile; the binding comes from the environment as in section 1) and, for Claude, two hook entries
  (`Stop` and `UserPromptSubmit`) for the native channel and the receiving check. Where the harness has
  its own command to add an MCP server (`claude mcp add --scope user`, `codex mcp add`,
  `opencode mcp add`) the engine runs it through `soyeht_process::run`, which is the harness's own way
  and needs no knowledge of its file format. Where it does not, or for hooks, the engine edits the
  file itself, atomically, keeping everything else, and refuses a file it cannot parse.
- **Where**, to be confirmed against each version in slice 7 (the screen shows the real path): Claude
  Code `~/.claude.json` (servers) and `~/.claude/settings.json` (hooks); Codex `~/.codex/config.toml` (or
  `CODEX_HOME`); OpenCode `~/.config/opencode/opencode.json`; Cursor `~/.cursor/mcp.json`, which Cursor
  also asks the person to approve (the consent screen says Connect does that, `agent mcp enable`). On
  **Windows** the same relative paths sit under `%USERPROFILE%` (OpenCode's and Codex's may follow
  `%APPDATA%` or their own home variables, and the engine reads the real one from the harness), the
  entry's command is the absolute path of `soyeht-mcp.exe`, a hook command is a launcher that exits 0
  when the program is gone, and quoting follows each harness's Windows rules. WSL-hosted harnesses are
  not connected.
- **Reversible.** The engine records, per harness, the file and the exact entry it added. Disconnect
  removes that entry, and only if it is unchanged; if the person edited it, the screen shows it and
  asks. `soyeht uninstall` disconnects every connected harness first. A hook entry whose program moved
  fails silently by construction (the launcher), and `status` shows stale entries.
- **Consent is UX, not a security boundary.** A program of the same user can write these files itself;
  the screen exists so that nothing is written behind the person's back and so they can see and undo
  what is.
- Mechanics: two new methods of the `harness` family (list the connections; connect or disconnect
  one), used by the apps and by `soyeht harness connect|disconnect|status`. The apps stay thin: the
  engine detects and writes, on the computer that runs the harness. A phone shows the state and asks
  the computer to connect; it never writes. The first-run offer is shown once and remembered in a
  setting; Preferences always has it.

### Per harness

All four are installed on the machine this was written on (Claude Code 2.1.325, Codex 0.164.0, OpenCode
2.2.5, Cursor Agent 2026.09.02, read from `--version` and `--help`, with no login and no configuration
touched). "Spike" marks what slice 0 must prove on the real binary.

| | Claude Code | Codex | OpenCode | Cursor Agent |
|---|---|---|---|---|
| Foreground | `claude` (a symlink into `versions/<ver>`) | `codex` | `opencode` | `cursor-agent` and `agent`: a script that runs `node` under its own name |
| MCP entry (connect) | `claude mcp add --scope user` | `codex mcp add` | `opencode mcp add` | `~/.cursor/mcp.json`, then approval |
| Binding reaches its MCP server? | expected yes | spike (shared app-server) | spike (background service) | spike (`persist`) |
| Fallback if not | `${VAR}` in the entry | `env_vars`; launch with `-c` | `environment`; launch with `--standalone` | `${env:VAR}`; launch flag |
| Typed channel | yes | yes | yes | yes |
| Confirmation | hook (exact bytes), at launch | screen; hooks after launch | screen; plugin after launch | screen; hooks after launch |
| Native channel | hooks (`Stop`, `UserPromptSubmit`), at launch | after launch | after launch | after launch |
| Screen | main by default (verify) | alternate by default (`--no-alt-screen`) | alternate (verify) | to measure |

Each harness draws its prompt differently, so the rule is generic and the difference is data. A profile
holds: how to find the input line from the cursor and the box or rule around it; the prompt glyph and the
forms of an empty line (a dimmed placeholder among them) and of a collapsed paste; the marks that mean
the line is not for text; the screen the harness draws on; and the range of versions it was verified for.
The four profiles are tested by one vector file and one procedure.

### What the data must show

The recorder shows, per pane, the rows around the cursor, the cursor, the alternate screen flag and the
time the screen stayed unchanged for each moment the input line went empty or not (`still` in its
analysis script). That is the instrument; this ADR fixes the form of the rule and what the data must
prove, as ADR 0069 does for a detector, before looking.

**Starting values** (limits in the contract, changeable without a protocol change):
`terminalQuietMs` 2000, `terminalSettleMs` 2500, `terminalEchoMs` 1000, `terminalConfirmMs` 8000,
`terminalEscalateSec` 60, `terminalMessageTtlSec` 86400. A person's gaps inside a burst of keys are well
below a second, so 2 s ends a burst; people also pause for seconds in the middle of a sentence, which is why the quiet time is
**not** what protects half-typed text (the non-empty input line is, rule 4): it only covers the moment
before the first key, which cannot be seen. The settle time must exceed twice the largest gap between
visible changes in a real turn, per harness: a clock that ticks every second makes 1500 ms too short.

**What to collect**, on a disposable engine with its own panes (never the owner's app; no input to the
owner's machine): a driver script feeds each of the four real harnesses at a pinned version through
`pane.input` while the recorder runs, on each of the three systems. Per harness: empty and idle; typed
text of several lengths, multi-line; a working turn (the gap distribution); a permission dialog; a
slash or file menu; history search; the editor mode line (Claude's vim mode and whatever the others
have); the external editor (the alternate screen); a pane resized; a **60000-byte paste and one at each
of 1022, 1024 and 4096 bytes** (what the harness receives, byte for byte, and how it shows it); the
harness reattached to a fresh client mid-session; and **adversarial screens**: a prompt box drawn by a
tool's own output, a dialog that appears inside the settle window, a static "running" line that never
changes, a harness that has exited with its last screen still showing. Claude's hooks give ground truth
for working and idle; for the others the driver does. The owner's own daily sessions are recorded for
a week by his choice (opt-in, local, private) for what a script cannot give: how real typing is paced.

**The bar**, fixed before collection and never lowered after it: over at least **1000 labelled
screens per harness**, the adversarial ones included, in which the line is not empty or is not an input
line, the rule calls **none** safe (zero false-safe: a miss is a message merged into the person's
words); every body size arrives byte-identical; and the median wait from an idle empty prompt to the
paste does not exceed `terminalQuietMs` plus `terminalSettleMs` plus the poll interval. The screens,
scrubbed of text, become `contracts/vectors/terminal-input-line.json`, and the engine's tests pass the
same file.

### Protocol changes (all additive, ADR 0161)

- `AgentMessageSource` (items of `agent.list`, source of a queued prompt): optional `kind`
  (`"agent"`, `"terminal"` or `"web"`, a value the engine sends, declared open), `workspaceName`,
  `canReceive`, `canReply` and a `reason`; the name is sanitised as above. `agent.list` and
  `agent.message` lose their same-workspace restriction.
- `agent.message` params: optional `targetName` beside `targetPaneId` (exactly one of them), and
  optional `inReplyTo` (a short message id).
  `AgentMessageResult`: optional `messageId` and `delivery`; `promptId` stays required and carries the
  message's id for a terminal recipient.
- `Pane`: optional `terminalHarness {id, name, version}` and the open messages of its inbox (state,
  sender source, queued time, preview, hop, attempts).
- **New methods**: the inbox **pull** and **ack** (a binding for its own pane, and the person);
  **message status** (the sender's view of its own messages); **policy** get and set (a person any,
  an agent its own pane, restricting only); and two of the `harness` family for the connections (list;
  connect or disconnect). Their dotted names are written out only when they land in the contract,
  because `check_doc_names.py` reads every dotted name.
- **One new event** of the pane family for the state of a terminal message, with the sender source, the
  preview and the reason.
- `welcome.capabilities`: `terminal-messages` and `agent-names`, offered on every system.
- `x-limits`: `terminalMessagesPending`, `terminalMessageTtlSec`, `terminalMessagesPerWindow`,
  `terminalQuietMs`, `terminalSettleMs`, `terminalEchoMs`, `terminalConfirmMs`, `terminalEscalateSec`,
  `terminalMessageAttempts`, `terminalMessagePreviewChars`, `terminalHopLimit`, `terminalLeaseSec`.
  `agentMessageBytes` is reused.
- No new error code: `not_found`, `invalid_params` (ambiguous name), `pane_busy`, `rate_limited`,
  `forbidden_role` (a policy denial, #169), `unauthorized` (a stale binding).
- Supervisor protocol 7 (ADR 0122): `Foreground`, and a guarded input. The pane's environment uses
  `Spawn.env`, which exists.
- CLI: `soyeht message`, `soyeht inbox read`, `soyeht harness connect|disconnect|status`. Contract 08
  (section 2's table: a terminal pane's binding; the known limitation), contract 04 R17, contract 17
  (helpers) and a new contract 19 (names, policy, the inbox, delivery, the profiles, the connection
  screen) change with the code, in the same pull requests.

### Per system

| | macOS | Linux | Windows |
|---|---|---|---|
| Binding in the pane | `Spawn.env` | the same | the pane process's environment block |
| Foreground program | `tcgetpgrp`, then the path, `argv[0]`, script | the same, from `/proc` | the pane's job tree, shallowest known harness, plus the profile match |
| Terminal model and typing | one code for all three; bracketed paste only | | markers through ConPTY are unproven: tested, no raw fallback for a message |
| Detection and connection files | `~`, Homebrew, npm | `~`, npm | `%USERPROFILE%`, `%APPDATA%`, `%LOCALAPPDATA%` |
| Harness hosting | native | native | native only; WSL, `tmux`, `ssh` and containers are plain terminals on every system |

Windows has the most new work (the job tree, the ConPTY probe, the paths and quoting), and it is in
the launch scope with the same slices, tested on the Windows VM.

### Tests

- **Unit.** The decision function is pure (observation, profile, clock in; wait, type or refuse with a
  reason out) and table-driven over `terminal-input-line.json`. The text reduction (controls, format,
  private-use and unassigned code points, the tag block, bidirectional controls, `\r`), the name rule,
  `native_text` with hostile names. Name resolution (case, accents, ambiguity, cross-workspace) and the
  four-layer policy. The inbox state machine: order, cap, 24 h deadline, claim and lease, retry only on
  evidence and at most 3 times, a claim by one channel skipping the other, a restart in each state. The
  caps (pair, aggregate, one in flight, hop pause). The binding: hash only, ends with the pane's
  process. Foreground matching on recorded process tables, including the `cursor-agent` and `claude`
  shapes. Paste mode tracking across a reset, a reattach and an engine restart.
- **Engine integration** (the runtime rig, `test-hooks`; never a production backdoor): a real supervisor
  and a small test harness that draws an input box, can switch bracketed paste on and off, clip an
  unframed paste at 1022 bytes (as the real one did), open a dialog and an alternate screen, and exit.
  `Hooks` supplies the clock and short thresholds. Cases: **the old failures as vectors** (a person pressed Tab, Up, Esc, Ctrl-U, Alt-Backspace, a mouse
  click or a menu key and the line is empty: the message is typed; text typed and deleted by any means;
  a slash picker opened and closed); delivery into an empty line; wait while text
  is in the line; wait during a keystroke, a dialog, the alternate screen and a version outside the
  range; mode unknown means nothing typed; a clipped attempt is detected and retried once with the same
  id; the harness exits between the check and the write (no text reaches the shell, no Enter); Enter
  withheld when the echo is wrong; two readers claim one message; a restart in each state; a
  cross-workspace message, a denied one and an ambiguous name; an unbound `soyeht-mcp` (no tools, no
  error); a message storm and a ping-pong to the hop pause.
- **Real harness**, the `e2e` full tier, with `scripts/e2e_real_agent_messaging.py` as the model:
  `scripts/e2e_real_terminal_messaging.py` runs **each of Claude Code, Codex, OpenCode and Cursor** in a
  terminal pane, started by the test and typed into by the test, connected through the consent flow (and
  disconnected after), on macOS, Linux and Windows. Per harness: an agent pane messages it by name
  across workspaces (it receives the whole text, acts and replies; again with text half-typed in its
  line: it waits, then delivers once the line is cleared; a 60000-byte body arrives byte-identical);
  it sends to an agent pane and to another terminal harness (where the spike says it can). **A waiting message is never silent**: with text left in a harness's line the
  strip, the sender's note at 60 s, the waiting count on the recipient's tool results and Deliver now
  all appear, and the sender's `message_agent` answer says queued and not delivered. **The owner's
  workflow is a test**: a Claude pane and a Codex pane in different workspaces, told "plan,
  review, at most two review rounds, then implement", run with no human input and finish with the
  expected message sequence. Claude also proves the hook path, the exact-byte rejection of a damaged
  paste and `stop_hook_active`. A harness that is not signed in is reported by name and fails the run,
  never skips it. The recorder runs the collection scenarios on the same rig.
- **Slice 0** is itself a test: the probe MCP server and the ConPTY paste probe run on all four
  harnesses and all three systems and leave a table, which this ADR's per-harness table is then
  corrected from.
- **Per system.** `scripts/check_engine_linux.sh` and `scripts/check_engine_windows.ps1` run the
  foreground, binding and connect code on their systems.

## Slices

Sizes are an agent's working days: S 1, M 2 to 3, L 4 to 6. The first two are small and land first.

| # | Slice | Size | Needs |
|---|---|---|---|
| 0 | **Spike**: the probe MCP server and shell-tool probe on Claude, Codex, OpenCode and Cursor started in a pane (environment, parents, controlling terminal), the ConPTY paste-marker probe, `--version` and foreground shapes of the real binaries. Gates every promise to send | M | |
| 0b | **`native_text`, the short header and the text reduction**: the compact `[from → to · m:ID]` header with the name rule, the removal of invisible code points on every channel, the forged-route fix and its tests; SECURITY.md and contract 08 state the known limitation | S | |
| 1 | **Sender, names and policy**: the pane binding at spawn on three systems (hash in the store, ended with the process), identity for terminal panes, `soyeht-mcp` terminal mode with a long-lived connection, `soyeht message`, names across workspaces with ambiguity, `kind`, the policy methods and their four-layer evaluation, the caps and the hop pause; the fallbacks the spike chose | L | 0 |
| 2 | **Foreground and listing**: `Foreground` (supervisor 7), matching on path, `argv[0]` and script, the Windows job tree, `agent.list` with terminals and web panes, `Pane.terminalHarness`, version read and cache | M | 0 |
| 3 | **Observation and decision**: the terminal model off the lock with its own paste-mode tracking, the profile type with Claude's profile, the pure decision function, the guarded input and the echo check, vectors from synthetic screens, ConPTY handling | L | 2 |
| 4 | **Calibration**: the collection scenarios on four real harnesses on three systems, the 1000-screen vectors, the paste-size runs, the thresholds, the other three profiles, held to the bar | L | 3, the recorder |
| 5 | **The inbox and delivery**: the store table, the states, the claim and lease, evidence-based retry, confirmation by screen, the watchdog and the notes to the sender, the waiting count on `soyeht-mcp` results, pull and ack, status, the event and the snapshot fields, the limits, generated outputs, contracts 04, 08, 17 and 19 | L | 1, 2, 3 |
| 6 | **Claude's hooks**: the native channel, the exact-byte receiving check, `stop_hook_active`, the 1.5 s ceiling and the launcher that survives a moved binary | M | 5 |
| 7 | **Connect**: detection and the writing and removal of each of the four harnesses' entries on three systems, the recorded entries, the two methods, `soyeht harness connect`, `soyeht uninstall` disconnecting | L | 1 |
| 8 | **The apps**: the consent screen (first run and Preferences), the message strip with Deliver now, Dismiss and the paused chain's Continue and Stop, the policy controls, on the Mac and the desktop; the state and the strip on the iPhone, iPad and Android | L | 5, 7 |
| 9 | **End to end**: `e2e_real_terminal_messaging.py` for the four harnesses, the cross pairs, the owner's plan-review-implement workflow on three systems, the Windows and Linux VM runs, `docs/platforms.md` | L | 4, 5, 6, 7 |

After launch: roles and orchestration graphs (the old app's planner, executor, reviewer, aggregator
templates, custom templates, and presets that close messaging to a graph and send each pane its role in
its messages), native channels for Codex, OpenCode and Cursor (each verified on its real binary), Pi, and
a refusal of a binding used from outside its pane.

Order: 0 and 0b first (0b needs nobody); then 1, 2 and 7 in parallel (7 needs only the binding's
shape); 3 as soon as 2 lands, with the recorder running at once so that 4 never waits on collection; 5;
then 6 and 8 in parallel; 9 last. The path that sets the date is 0, 2, 3, 4, 5, 9: calibration depends
on real harness runs and cannot be shortened by adding people.

**Estimate.** The earlier draft of this ADR (the doorbell and the pane-origin machinery) was reviewed at
about 2.5 to 3 weeks with 2 or 3 implementers. Dropping pane origin takes about a week off. The doorbell
version took another half week off again, and this version puts it back: full-message typing needs the
paste-mode tracking, the confirmation and the retry, and the old app's names, cross-workspace policy and
caps are now in scope. The result is **about 2.5 weeks with three implementers**, no shorter than the
reviewed draft, with Windows at launch and the connection screen inside it. Calibration and the
Windows VM runs decide the end, not the amount of code.

If the date presses, in this order: the apps beyond the Mac and the desktop (the iPhone, iPad and
Android show nothing until later; the message still works), the Windows e2e depth (the unit and
integration tests stay), the Preferences copy of the connection screen (first run stays), the policy
controls in the apps (the defaults and the tool stay). Receiving for all four harnesses, Claude's
sending and the retry and confirmation rules are not cut.

## Alternatives considered

- **A doorbell: type a short line and let the harness fetch the body.** Rejected by the owner's wish
  that the recipient gets the whole message and acts, as in the old app. (The old app does use a short
  bootstrap, but for conversation handoffs, not for messages.)
- **Count keystrokes to know whether the person is typing.** It is what the old app does, and it is
  what wedged its queue; the screen is the truth the engine can read.
- **Type whenever the input has been quiet.** A pause is not an empty line.
- **At most once.** It is the old app's choice, and it is where "gets lost" comes from. A rare visible
  duplicate is accepted over a silent loss.
- **A file spool and a heartbeat.** The engine already has a connection, a store and events.
- **Classify each connection by the process behind it, and refuse the person's powers to panes.** The
  owner declined it for launch (Scope); the command-line path is a documented limitation.
- **A shim of `claude` on the pane's `PATH`.** It changes what the person runs and breaks for an alias
  or an absolute path.
- **Per-launch flags for every harness.** The connection screen replaces them; they remain only as a
  spike-chosen fallback for a harness whose MCP server does not see the pane's environment.
- **A wrapper that hosts every harness behind a pty of its own.** It would make the harness a child of
  Soyeht's program and replace four TUIs' behaviour with ours.

## What this does not stop

- Any process with a pane's environment acts for that pane (section 1). The command-line path to agent
  panes is the known limitation above.
- A message can still be wrong in the moment between the last check and the write, and then leaves one
  visible paste with no Enter; it cannot send what the person typed or what a shell would run.
- A harness in WSL, `tmux`, `screen`, `ssh` or a container is a plain terminal; a harness version
  outside its profile's range waits for the person's Deliver now.
- A rare duplicate: an attempt the engine could not prove landed is repeated, and says so.
- A harness started with its permissions off receives a message like any other: this ADR makes no
  distinction (Scope). A message is text another agent read somewhere; the harness's own approval
  prompts, if it has them on, are the protection, as for any prompt.
- Agents talking to each other without a person is the point; the caps and the policy are the person's
  brakes, and the pause at hop 24 is the last.
- Consent to connect is not a security boundary (section 4).

## Open questions

1. **A message addressed to a web pane.** A web pane has no input and no harness. The proposal: it is
   listed by name so agents can find and edit it, and a message to it goes to the agent pane that created
   it, when the engine records one, else `not_found`. Is that what you mean by messaging reaching web
   panes?
2. **Roles and orchestration at launch?** The old app has role templates and graph presets. This ADR
   leaves them for after launch: you give each pane its personality in plain words, as in your example.
   Say if the planner/reviewer presets must ship now.
3. **A rare duplicate instead of a loss.** When the engine cannot prove a message landed, it sends it again
   marked `repeat`. Is that the trade you want?
4. **Cursor asks its user to approve a new MCP server.** The consent screen says Connect does that for
   the person. Acceptable, or should Soyeht only tell them to approve it in Cursor?

Everything else that was open is decided above, or waits on the spike (what each harness hands to its
MCP server) or on the recordings (the thresholds and the profiles).
