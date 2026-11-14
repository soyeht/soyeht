# ADR 0165: A message reaches a harness that runs in a terminal pane, and a program in a pane is never the person

- Status: proposed (phase 1: the design only; nothing here is built; the owner decided on 2026-11-14 that
  all three parts land before 0.1.0)
- Date: 2026-11-14
- Builds on ADR 0148 and contract 17 (agents message and delegate to agents), contract 04 R17 (the
  attributed queued prompt and the full-access refusal, #169), contract 08 section 2 (roles), ADR 0069
  (what a terminal pane's screen can be observed for), ADR 0161 (the protocol only grows), ADR 0162,
  0163 and 0164 (what the engine can tell the app from on each system), ADR 0079 (nothing blocks
  under the Core lock), ADR 0117 (a typed input is never repeated), ADR 0091 (no test backdoors) and
  ADR 0122 (a change of the supervisor counts for an update).

## Context

A harness (Claude Code, Codex, OpenCode, Cursor, Pi) runs in one of two ways. As an **agent** it is a pane
that Soyeht opens over ACP, with the Soyeht MCP, and `agent.message` between such panes works: it is
attributed, limited to 6 a minute per recipient, refused into a pane in full access (#169), and held
until the recipient's queue can take it. In a **terminal** the person types `claude` (`codex`,
`opencode`, `cursor-agent`, `pi`) in a terminal pane. The owner uses Claude Code that way every day, and
both kinds must be able to receive and to send, for **all five harnesses equally**: nothing in the design
is Claude's alone, and the one place where Claude is first (its hooks already exist) is a matter of
order, not of scope (section "Per harness").

An end-to-end run on origin/main 2dc4d381 (2026-11-14) found four things:

1. **A terminal pane is invisible to messaging.** `agent.list` lists only `PaneMode::Agent` panes,
   and `agent.message` to a terminal answers "Target agent pane not found"
   (`agents/methods.rs`, `agent_peer_method`). Nothing is typed. Typing is mechanically possible
   (`pane.input`: bracketed paste, a short pause, `\r`), but a manual test merged the message into
   half-typed text in the harness's input line.
2. **A terminal harness cannot send.** `soyeht-mcp` needs a binding that only an ACP session gets
   (`agents/mcp.rs`, `dispatch.rs` `agent_identity`, which looks in `self.agents` only).
3. **Any program in any pane is the person.** A local connection with no binding is `human_local`.
   `soyeht pane prompt ID TEXT` (and a raw call on the socket, which is the same thing without the
   command) therefore reaches an agent pane in full access as the unattributed person, and goes round
   #169. It is worse than the prompt: `pane.input` types into any terminal (a terminal harness started
   with its permission checks off included), `pane.permission.answer` approves what an agent asked,
   `pane.config.set` raises a pane's mode, and every place that asks `Role::is_person`
   (`require_person_for_full_access`, task movement, roles, worktree cleaning) believes it.
4. **Turn boundaries are known only sometimes.** `pane.create` with `mode=terminal` and a harness
   adds `--settings` hooks for Claude (`terminal_harness.rs`, `agents/hooks.rs`) that report a prompt
   submitted, a permission asked, a tool used and the turn's end. They exist only when Soyeht started
   the harness, they are installed for the experiment's labels and are a no-op while no capture runs,
   and they exist for Claude alone. The four other harnesses have no Soyeht hook at all, and one of them
   (Pi) has no MCP.

The screen-state recorder (#221, `docs/research/terminal-screen-recorder.md`) was built to find out
when a terminal's input line is safe to type into. **No recording exists yet** (the run worktree holds
none, and the recorder's default folder is absent on this machine), so every threshold below is a
starting value with its reason, and the section "What the data must show" says what to collect and
the bar the thresholds must meet. The owner rejected rules that count keystrokes or know what an
editor's keys mean (a Vim `dd` changes text without typing it). The decision is made from the
rendered screen of the input line and the person's recent input, or the message is not typed at all:
it waits in an inbox that the harness reads at a turn boundary.

## Decision

Four parts, in the order they land.

1. **A connection says where it comes from** (the "pane origin"), and a connection that comes from
   inside a pane is never the person. This closes the bypass and gives a terminal harness an identity
   with no secret to leak.
2. **The sender**: a terminal harness sends with the same `soyeht-mcp` and the same limits, identified
   by its pane origin (Pi, which has no MCP, with an extension; `soyeht message` for any).
3. **The recipient**: a terminal pane whose foreground program is one of the five known harnesses is
   listed with its own kind, holds messages in a durable queue and receives them through one of two
   channels: the harness's own native channel at a turn boundary (a hook, a plugin or an extension;
   no typing), or a typed paste when, and only when, the rendered input line is provably empty and
   quiet. The difference between the five TUIs is data (a profile each), not code.
4. **Additive protocol**, no new method, no change to an existing field (ADR 0161).

### 1. The pane origin and the person

#### What the engine learns about each local connection

When the engine accepts a connection on the local endpoint (where it already learns the peer's user
and, on macOS, whether it is the signed app) it also asks who the peer process belongs to, and keeps
the answer on the connection:

- **`Pane(paneId)`**: the peer is inside a process set that the supervisor started for that pane: a
  terminal pane's shell and everything it ran, an agent's adapter and the commands and terminals the
  agent started, a helper agent's.
- **`Outside`**: the supervisor says the peer is in none of them.
- **`Unknown`**: the question failed (the supervisor is old or away, the process is gone, the system
  refused). It counts like `Pane` for every decision below that restricts, never like `Outside`: the
  engine fails closed, and logs one line with a kind and no path (the `origin_unknown` log event).

The question goes to the **supervisor** (`soyeht-ptyd`), not to the engine's own process table
code, because the supervisor started the processes: it knows each instance's session leader and, on
Windows, holds each pane's job object, which the engine does not. One new request of the supervisor
protocol, `Whose {pid, started}`, answers with the instance key or nothing, and `soyeht-platform`
holds the per-system process code behind one public shape. It is asked at accept, off the Core lock
(the listener's `spawn_blocking` that already attests the app, with a clone of the supervisor link
taken for an instant), so it costs the connection one local round trip, once. The supervisor
protocol version goes from 6 to 7 (ADR 0122); an engine that finds an older supervisor treats every
connection as `Unknown` and says so in the log.

How the supervisor decides, on each system:

| | macOS | Linux | Windows |
|---|---|---|---|
| The peer process | `LOCAL_PEERTOKEN` audit token: the pid and its generation, so a reused pid is not mistaken | `SO_PEERCRED` pid (and `SO_PEERPIDFD` where the kernel has it, which cannot be reused) | the owner of the loopback TCP row of the accepted connection, read at accept (the same lookup ADR 0162 names for the app's signature) |
| In a pane? | the peer's session id is an instance's session leader (every pty child and every adapter guardian calls `setsid`), or an ancestor of the peer is an instance's process; every step of the walk compares start times, so a recycled pid breaks the chain instead of extending it | the same, from `/proc` (`stat`: parent, session, start time); where the supervisor can set `PR_SET_CHILD_SUBREAPER`, a pane's orphans come back to it instead of to `init`, so "descends from the supervisor" holds for a process that was orphaned on purpose | `IsProcessInJob` against the job the supervisor made for each pane. A child cannot leave a job that allows no breakaway, which `soyeht-platform` already does not allow |
| What escapes | a process that calls `setsid` and loses its parent (a daemon, `launchd`, `cron`, a LaunchAgent a pane wrote, `tmux`'s server) | the same, and `systemd-run --user`, `at`, cron | a process started by the Task Scheduler, WMI or a service on a pane's behalf |

An escaped process is `Outside`. The rule of the next section takes care of what that means for the
person on macOS; on the other systems it is the limit that ADR 0162 and ADR 0164 already state: the
engine cannot tell a program of the same user from the app. A harness run inside `tmux`, `screen`,
`ssh` or a container is likewise not recognised (its processes are not the pane's), and the pane is a
plain terminal for messaging. This is a stated limit, not a defect to chase.

#### Who counts as the person

- The **signed Soyeht app** (macOS, `signed_ui`) is the person, whatever its ancestry.
- A **paired device** is the person (ADR 0013), as today.
- A local connection of origin `Outside` is the person, as today, except for the one case below.
- A local connection of origin `Pane` or `Unknown` that is not the signed app **is not the person**.
  `Connected::person()` says so, and it is the one answer: the seven production calls of
  `Role::is_person` ask it instead (the wire `Role` is unchanged; a test fails when a production
  call of `is_person` remains outside it).

A connection that is not the person **acts for its pane**: it gets the agent column of the role
matrix (contract 08 section 2.4) for everything that changes anything, and the human column for
everything that only reads. A refused call answers `forbidden_role` with a message that says why and
what to use ("A program started in a Soyeht pane acts for that pane and not for the person. To send
a message to an agent use the message command or the message_agent tool"). The set that must be
refused is the set that reaches an agent or a terminal as if it were the person, or raises an
agent's authority: `pane.input`, `pane.prompt`, `pane.cancel`, `pane.permission.answer`,
`pane.elicitation.answer`, `pane.config.set`, `pane.harness.switch`, `pane.queue.remove`,
`pane.queue.resume`, `pane.role.set`, the `pane.auth.*` methods, the task methods that dispatch,
unblock or move on the person's word, `harness.custom.*`, `skills.*`, `settings.set`,
`project.trust.set`, and `pane.create` or a task with a full-access mode (through
`require_person_for_full_access`, which now asks `person()`). Closing, renaming, creating a pane
that is not in full access and every read stay as they are, so a person who types
`soyeht pane list` or `soyeht pane close` in a Soyeht pane is not disturbed. The exact membership is
a table in contract 19, and a contract test fails when a method that changes state is in neither
column, so a method added later cannot forget to choose.

`soyeht pane prompt` from a pane is refused, and a new command, `soyeht message PANE TEXT`, sends the
same text as an attributed `agent.message` from the pane the connection comes from. That is the
"attribute it as a pane message" option, and it is the only way a pane's program reaches another
pane. It needs no flag and no binding.

On macOS there is one more rule, because there the engine can attest the app: **a prompt, an input or
an approval into a pane in full access needs the signed app (or a paired device)**, so a program that
escaped its pane's process set, or was started by `launchd` on a pane's behalf, is refused too. The
cost is that `soyeht pane prompt` from Terminal.app cannot drive a pane that approves everything; it
still drives any other. An engine that cannot attest (built with `cargo build`, which logs
`engine.attestation_unavailable`) does not apply this rule, as it does not apply ADR 0162's.

#### What this does not stop

- On Linux and Windows, a program that left its pane's process set is `Outside`, and an `Outside`
  connection is the person, because nothing on those systems tells it from the app (ADR 0162's open
  question; ADR 0164 chose the person's code for pairing, which does not fit a prompt). The rule
  stops the program an agent runs from a shell tool, a script in a pane and a pane a paired device
  opened, which is how the bypass is reached in practice; it does not stop a program that goes out of
  its way, on Linux with `systemd-run` or `setsid`, on Windows with the Task Scheduler. On Windows the
  job makes leaving a pane's set much harder than on Linux; on macOS the full-access rule closes the
  path for the case that matters most.
- A same-user program that writes the engine's database, or injects into the person's app, or into
  a process of the pane (ptrace, `task_for_pid`: macOS and Yama's default on Linux refuse these for a
  process that is not a descendant or has no entitlement).
- The desktop app on Linux and Windows is a program like any other: started from a Soyeht pane it
  would be that pane's child and lose the person's powers. It detaches from the process set it
  was started in when it starts (a new session on Unix; the app's own start is where this lives).
  The macOS app is the signed app and is not affected.
- A binding, which an ACP agent's `soyeht-mcp` receives by environment, is still valid from anywhere
  as today. The engine logs when one is used from outside its pane (a `binding_origin` log event) and
  does not refuse yet: a harness that starts its MCP servers through a launcher that daemonises would
  break. Refusing is a follow-up once the log shows it is safe.

### 2. The sender

A terminal harness gets its identity from the **pane origin**, not from a secret. The reasons:

- A secret in the pane's environment is inherited by everything the pane runs (including the commands
  the harness's own shell tool starts, which is where the danger is), and `ps` shows a process's
  environment to its own user. A secret in a config file is a file any program of the user reads.
- The kernel's word about where a process runs cannot be copied. A process in pane Q has no way to
  claim pane P, and one that left every pane has no pane to claim; nothing is stored, rotated or
  leaked. Inside pane P, any process of P acts for P: that is the limit, and it is the same limit a
  secret would have.
- It makes the MCP configuration the same for every pane, so it can live in the harness's own
  configuration and work when the person types `claude` themself.

How a harness gets the way to send (the exact mechanism of each is in the table of "Per harness"):

- **A harness Soyeht starts** (`pane.create` with `mode=terminal` and a harness): the engine adds
  what the harness takes per launch (a flag, a `-c` override, an environment variable or an
  extension path), with no secret in it and nothing written to the person's configuration.
- **A harness the person starts**: Soyeht offers to connect it, once per harness, and never does it
  silently. `soyeht harness connect claude|codex|opencode|cursor-agent|pi` shows what it will change
  and then uses the harness's own documented command or file to add the MCP server (or, for Pi, the
  extension) at user level, writes nothing else, and `soyeht harness disconnect` and `status` undo
  and show it. Preferences has the same offer. A harness whose configuration cannot be written
  without a prompt gets the exact text to add instead. The server has a name of its own (not
  `soyeht-workspace`, which an ACP agent pane declares) so a pane that loads both is never
  ambiguous, and a Soyeht server of another profile or a legacy one is never touched.
- `soyeht-mcp` gains a mode with no pane arguments and no binding. Its engine connection says
  `client: soyeht-mcp` as today; the engine, which has the connection's pane origin, grants a
  connection of origin `Pane(P)`, where P is a terminal pane that runs a known harness, the agent
  role for `agent.list` and `agent.message` only. Task tools stay with the ACP agents in this phase:
  they carry a task attempt that has nothing to attach to in a terminal.
- **Pi has no MCP** (its own documentation says so, by design). Its way to send is an **extension**,
  a TypeScript module Pi loads in its own process, that registers the same two tools
  (`list_agents`, `message_agent`) with `registerTool` and talks to the engine's local socket with
  the same pane origin (the extension runs inside Pi, so inside the pane). The extension is one
  small file shipped with Soyeht, with no dependency, and it is also Pi's receiving channel
  (section 3). Where a harness has a shell tool, `soyeht message` (section 1) is the universal
  fallback that needs no configuration at all: a harness that has neither MCP nor extension can
  still send by running the command, and the instructions every connection adds to the harness
  (the `soyeht-mcp` instructions, the extension's tool descriptions) say so.

Limits are the agents' and are not copied: the sender's name and pane id travel in the same
`AgentMessageSource`; `message_times` and `AGENT_MESSAGES_PER_WINDOW` give 6 a minute per sender and
recipient; the recipient must be in the sender's workspace; the full-access refusal applies to a
terminal recipient (section 3, "The recipient's mode"); and the sender gets the same
`rate_limited`, `not_found` and `forbidden_role` answers. A terminal sender can reach an agent pane
(the existing queue, with `kind` saying "terminal" on the source) and another terminal.

### 3. The recipient

#### Detecting a terminal harness

A terminal pane has a harness when the **foreground program of its terminal** is one. The supervisor
reports it (one more request, `Foreground`, answered for a list of instances; on Unix the process
group of the pty from `tcgetpgrp`, on Windows the newest non-console leaf of the pane's job, which is a
heuristic and is documented as one). It names the program (the executable's name, and the script it
was given when the executable is `node` or an interpreter) and its arguments. A table in the engine
maps `claude`, `codex`, `opencode`, `cursor-agent` (and `agent`, the same program under its short
name) and `pi` to a harness id and its **profile** (below).
The engine asks on a slow tick (2 s, for the terminal panes of a workspace that has a pane waiting or
a sender that listed) and on demand, never in a loop that costs a pane that runs a shell.

A pane that Soyeht started with a harness is known with certainty by its launch record, and the
foreground check still runs: a harness the person left (`/exit`, Ctrl-Z) or one that runs under a
launcher that daemonised no longer holds the terminal, and then it is not a recipient.

`agent.list` lists such a pane, with `kind: "terminal"` on the item and the harness's display name,
and `Pane` carries an optional `terminalHarness {id, name}` for the apps' header. A pane whose
foreground is a shell, an editor or an unknown program is not listed, so a sender never sees a
recipient that cannot take a message.

#### The queue

A message to a terminal recipient is stored in a **durable inbox per pane** (the store, one table;
`soyeht-store` is the only crate with SQL), not in the agent prompt queue, which has no meaning for a
pane with no session. `agent.message` returns at once with `queued: true` and a `messageId` (in
`promptId`, which the contract requires, and in an additive `messageId`) and `delivery: "terminal"`.
The inbox holds at most `terminalMessagesPending` (8) per pane; a ninth is refused with
`pane_busy`. Messages go in order, one at a time: after one is delivered the next waits for the
harness to be idle again, so a backlog never arrives as a block. Each has a deadline,
`terminalMessageTtlSec` (900 s from the time it was queued). The same idempotency receipts, the
same `message_times` table and the same 6 a minute apply as for agents; the size limit is
`terminalMessageBytes` (8192), smaller than an agent's 60000, because a typed paste that the harness
collapses cannot be checked line by line.

A restart keeps the inbox. A message that was `delivering` when the engine stopped is not typed again
(ADR 0117's rule): it becomes `needs_you`.

#### Two channels

Each message goes through the first channel that is open.

**A. The native channel: no typing, for a harness that has a turn hook, a plugin or an inbox of its
own.** The harness's own extension point asks the engine for the pane's pending messages and hands them
to the harness the way the harness takes input, so the person's half-typed text is never touched. What
each harness offers is in the table of "Per harness"; the shape is the same for all:

- **at the end of a turn**, the message starts the next one at the earliest boundary a working
  harness has (Claude's `Stop` hook as the reason to continue; Pi's `agent_settled` event and
  `sendUserMessage` when Pi is idle; a plugin's idle event in OpenCode);
- **when the person submits a prompt**, as added context where the harness has such a hook (Claude's
  `UserPromptSubmit`), so a message that arrived while the harness was idle is not missed when the
  person is about to talk to it anyway.

The program on the harness's side marks a message `delivered` only when it says it handed it over.
Every mechanism is documented behaviour of its harness at the version checked, and the e2e test pins
it on the installed version; a version that stops honouring it makes the channel report itself
closed (no acknowledgement within the deadline leaves the message queued for channel B). The
program comes with the launch for a harness Soyeht starts, and from the user-level configuration of
`soyeht harness connect` otherwise. It uses the pane origin, does nothing at all outside a Soyeht
pane, and gives up when no engine answers within 1.5 s, so it never slows a harness that runs
elsewhere. Today only Claude's hooks exist in Soyeht, installed for the experiment's labels and a
no-op without a capture; they now carry the inbox too and are always on.

A native channel cannot reach a harness that sits idle at its prompt unless the harness's extension
runs on a timer or an event (Pi's does, a Claude hook does not: none fires). For that case, and for a
harness with no such channel, there is channel B.

**B. The typed channel: a paste into an idle, empty, quiet input line.** The engine types the message
as one bracketed paste and then Enter, only when all of the following hold, in this order. The
conditions are about what the engine can observe; none of them counts or interprets a keystroke.

1. **Recipient.** The foreground program is still the harness, the pane has a process, no harness
   switch or restart is pending, and the harness is not in full access (below).
2. **The person is not at the keyboard.** At least `terminalQuietMs` (2000 ms) since the last
   `pane.input` the engine accepted for this pane, from any client. The engine sees all of them (it
   makes their durable receipt) and keeps only the time.
3. **The screen has settled.** The pane's rendered screen has not changed for `terminalSettleMs`
   (1500 ms). A harness at work redraws a spinner or a running clock each second, so a quiet screen is
   also the sign of an idle turn for a harness without a native channel; where there is one, the turn
   must also be idle by its last event (a prompt submitted with no end of turn after it means
   working).
4. **The input line is the harness's, and it is empty.** The engine keeps a terminal parser for the
   pane (the same terminal model as `terminal_projection`, fed off the Core lock through a bounded
   queue as the experiment's capture is; a queue that overflows marks the screen unknown until it is
   rebuilt from the pane's retained output). The harness's **profile** says where the input line is on
   the rendered screen (relative to the cursor and the box the harness draws), what an empty one
   looks like (the prompt glyph and nothing the person wrote; a dimmed placeholder is not text), and
   which marks of the screen mean the line is not for text: a permission dialog or a menu, a history
   search, a mode line that says the editor is not inserting (Claude's vim mode shows `NORMAL`), an
   alternate screen that the harness does not use at that moment (each profile says whether the
   harness draws on the main or the alternate screen: Codex's help documents an option to turn its
   alternate screen off, Pi has a regular and a fullscreen mode, so the value is the profile's and
   the data's, never a rule of the engine; an editor or pager opened from the harness flips it and is
   then not the harness's line). The decision is **about the screen, not
   about the editor**: a Vim `dd` in the person's editor is irrelevant because the line is empty
   and nothing is half-typed, and an editor mode the harness draws is a mark on the screen that a
   profile names. One generic procedure reads every profile; a profile is data (anchors, glyphs,
   placeholder forms, marks), so five different TUIs need five profiles and no code per editor.
   When the profile cannot say "empty", the answer is "not safe", never "probably".
5. **The terminal takes a paste.** The program has switched bracketed paste on (the parser knows it).
   Without it a newline would be Enter, so the message is not typed (it stays for channel A or
   expires).
6. **The text is clean.** The engine, not the sender, builds the bytes: the text is reduced to
   printable characters, newline and tab (every other control character, escape, and the 8-bit
   controls are replaced with U+FFFD, `\r` becomes a newline), so a message cannot contain the
   end-of-paste marker or any other escape that would turn the rest into keystrokes. The engine adds
   the paste markers.

The typing itself is two steps, so that nothing the person does between the check and the write can
be sent:

1. **Paste.** One supervisor input, with a guard that the supervisor checks at the instant of the
   write: the pane's foreground process group is still the one the engine decided on. (If the harness
   exited a moment ago, the text would land in a shell; the guard refuses it.) The write is
   `ESC [ 200 ~`, the text, `ESC [ 201 ~`, with no Enter.
2. **Verify, then Enter.** Within `terminalEchoMs` (1000 ms) the screen must show the harness's input
   line holding exactly the pasted text, or the harness's collapsed form of it (Claude shows
   `[Pasted text #1 +N lines]`; the profile says the form). Only then the engine sends Enter, and it
   confirms within `terminalSubmitMs` (5000 ms) that the input line emptied or the harness shows it
   works. If the line shows anything else (the person typed in between), or the foreground changed,
   **no Enter is sent**: the message becomes `needs_you`, the pasted text stays where it is, and the
   person is told (below). The attribution line at the top of the pasted text makes a leftover
   obvious. Nothing the engine does can undo a paste in a harness's line, so the conditions above
   are the safety, and this step only limits the damage of the race between the last check and the
   write to one visible paste that nobody sent.

The text typed is the one that agents already receive, with its reply route (`agents/peer.rs`
`native_text`; the server name is the recipient's), so the person reads in the harness who wrote it.
One function builds it for both kinds of recipient (one rule, one place).

#### When it never becomes safe

The message waits for channel A or B until its deadline. Then it is `expired` and removed. The engine
tells the sender with the same mechanism a delegated helper's completion uses (contract 17): a prompt
queued in the sender's own pane (or its inbox, if the sender is a terminal), written by the engine
and not by anyone else, so it is safe even for a sender in full access: "Your message to NAME was not
delivered: the person was typing in its input line for 15 minutes." The reason is a closed set
(`typing`, `busy`, `unknown_screen`, `not_a_harness`, `full_access`, `dismissed`). The person can
also dismiss a waiting message from the app, which tells the sender the same way. A message is never
typed because time has passed: the input-line conditions have no timeout that relaxes them.

#### The recipient's mode

#169 refuses an agent's message into a pane in full access because a pane that approves everything
acts on text an agent read somewhere. A terminal harness's mode is not known the way an ACP pane's
is, so the engine says what it can see, and is stricter where it cannot see:

- the arguments of the foreground program (the flags of each harness that skip its permission
  checks, in the table of "Per harness") and
- the profile's mark on the rendered screen (Claude's status line under the input says when it
  approves everything).

Either one means full access. The check is made when the message is queued (the sender gets
`forbidden_role`, "An agent cannot send a message to a pane that is in full access") and again at
every delivery attempt (the mode can change while a message waits; the message expires with reason
`full_access`). A harness whose mode shows in neither place is treated as not in full access, which
is the one place this design trusts the absence of a sign; the profile of each harness says whether
it has a sign, and the recorded screens must contain the full-access state of every harness that has
one before the profile is accepted.

**Pi has no approval step at all**: its own documentation says it has no permission popups and no
sandbox, and runs with the user's permissions. By the reasoning of #169 a Pi pane is always in full
access, so an agent's message into it is refused the same way, and this ADR does not weaken that on
its own. Pi can still send, and can receive from the person. The owner asked that Pi receive too, so
there is a person-only switch for it, off by default: "Let agents message this pane", per terminal
pane, which only the person can set (it is in the person-only table, so a program in a pane cannot
turn it on) and which records that the person chose to let text another agent read reach a harness
that approves everything. It exists for every terminal pane in full access, not for Pi alone, and
does not touch #169 for ACP panes. It is an owner decision (open question 1).

#### What the person sees

Every message is visible twice. In the **harness**, for typed delivery, the pasted text starts with
the attribution (who, which harness, which pane) and is submitted as the person's next prompt; the
harness's own transcript shows it. For hook delivery the text goes into the harness's context, where
the person may not see it, so the apps show a **strip under the pane's header**: "Message from NAME
(Claude Agent)", the time, its state (waiting, delivered, needs you) and the first
`terminalMessagePreviewChars` (160) characters, with Dismiss. It is flat, in palette roles and the
tokens of the shared sections, like every pane interior. The strip is driven by a new event with the
message's state and by the snapshot (open messages are part of the pane), so a client that reconnects
shows it. The event carries the preview and not the text.

### Per harness

All five are installed on the machine this was written on; what follows was read from each one's
`--help` and, for Pi, its shipped documentation, with no login and no configuration touched (Claude
Code 2.1.325, Codex 0.164.0, OpenCode 2.2.5, Cursor Agent 2026.09.02, Pi 0.84.1). Where a row says
"verify", the mechanism is a candidate that slice 8 must prove with the real binary before the
harness reports the capability; a harness whose native channel does not verify keeps the typed
channel, which needs nothing from the harness.

| | Claude Code | Codex | OpenCode | Cursor Agent | Pi |
|---|---|---|---|---|---|
| Foreground program | `claude` | `codex` | `opencode` | `cursor-agent`, `agent` | `pi` (a script run by `node`: the engine reads the script's name) |
| Send, per launch (Soyeht starts it) | `--mcp-config` | `-c` override of `mcp_servers` (TOML, documented in its help) | its configuration through a file or the environment (verify) | a plugin folder with `--plugin-dir` (verify) | `-e` with the Soyeht extension |
| Send, user level (`soyeht harness connect`) | `claude mcp add --scope user` | `codex mcp add` | `opencode mcp add` | `~/.cursor/mcp.json`, then the person approves it (`agent mcp enable`; `--approve-mcps` is not used) | `pi install` of the extension; **no MCP exists in Pi** |
| Send, fallback | `soyeht message` from its shell tool | same | same | same | same, and the extension's own tools |
| Receive, native channel | hooks: `Stop`, `UserPromptSubmit` (`--settings` per launch; user `settings.json`) | `hooks` feature (stable; the person must trust the hook, so `connect` says so); candidate: `codex queue` to the shared local app-server (verify that a TUI session is reachable) | a plugin on its idle event (`opencode plugin add`), or its local server API (verify) | its hooks or plugin (verify) | extension: `agent_settled`, `isIdle`, `sendUserMessage`: delivered as a user message, no typing, whatever is in the editor |
| Full-access signs | `--dangerously-skip-permissions`, a bypass permission mode; the status line | `--dangerously-bypass-approvals-and-sandbox`, `-a never` with a danger-full-access sandbox | `--auto`; permission configuration (verify) | `-f`, `--force`, `--yolo`, sandbox disabled | **always**: no approval step exists; see "The recipient's mode" |
| Screen | main screen by default (verify) | alternate screen by default (`--no-alt-screen` turns it off) | alternate screen (verify) | to measure | main screen, or fullscreen mode |

Each harness draws its prompt differently, so the rule is generic and the difference is **data**. A
profile holds: how to find the input line from the cursor and the box or rule the harness draws
around it; the prompt glyph and the forms of an empty line (including a dimmed placeholder); the
forms a paste is shown in (verbatim, or collapsed); the marks that mean the line is not for text
(a dialog, a menu, a search, an editor mode line, an unexpected screen); the screen the harness
draws on; and the signs of full access. The five profiles are tested by one vector file and one
procedure. Pi's native channel makes its profile matter less: a message sent through the extension
is not typed at all, so the typed channel is Pi's fallback only for a Pi started without it.

### What the data must show

The recorder shows, per pane, the rows around the cursor, the cursor, the alternate screen flag and
the time the screen stayed unchanged for each moment the input line went empty or not
(`analyze.py`'s `still`). That is the instrument; this ADR fixes the form of the rule and what the
data must prove, as ADR 0069 does for a detector, before looking.

**Starting values** (each a limit in the contract, changeable without a protocol change):
`terminalQuietMs` 2000, `terminalSettleMs` 1500, `terminalEchoMs` 1000, `terminalSubmitMs` 5000,
`terminalMessageTtlSec` 900. Reasons: a person's gaps between keys inside a burst are well below a
second, so 2 s ends a burst; but a person also pauses for several seconds in the middle of a
sentence, and this is why the quiet time is **not** what protects half-typed text (the non-empty
input line is, rule 4), and it only covers the moment before the first key, which cannot be seen at
all. The settle time rests on a harness that redraws about every second while it works, which is
the thing to measure: the largest gap between visible changes during a real turn, per harness.

**What to collect**, with a disposable engine and its own panes (never the owner's app, and no
input to the owner's machine): a driver script feeds each of the five real harnesses at a pinned version
through `pane.input` while the recorder runs. Per harness, the scenarios:
empty and idle; typed text of several lengths, multi-line; a working turn (the gap distribution
during it); a permission dialog; a slash or file menu open; history search; the editor mode line
(Claude's vim mode, normal and insert, and whichever the others have); the external editor (the alternate screen); a 10 KB paste
(what the harness shows for it); a pane resized; and each harness's full-access state. Claude's hooks and Pi's
extension events give ground truth for working and idle; for the others the driver's own script does. The owner's own
daily sessions are recorded for a week by his choice (opt-in, local, private) for the one thing a
script cannot give: how real typing is paced and what the screens look like in real use.

**The bar**, fixed before collection and never lowered after it: over at least 200 labelled screens
per harness in which the line is not empty or is not an input line, the rule must call **none**
safe (zero false-safe; a miss is a message merged into the person's words), and the median wait from
an idle empty prompt to the paste must not exceed `terminalQuietMs` plus `terminalSettleMs` plus the
poll interval. The screens, scrubbed of text, become `contracts/vectors/terminal-input-line.json`
(a screen, the harness, the expected answer), and the engine's tests pass the same file.

### Protocol changes (all additive, ADR 0161)

Nothing existing changes or is removed; there is no new method.

- `AgentMessageSource` (the items of `agent.list` and the source of a queued prompt): optional `kind`,
  `"agent"` or `"terminal"` (the value of a field the engine sends, declared open).
- `AgentMessageResult`: optional `messageId` and `delivery` (`"prompt"` or `"terminal"`); `promptId`
  stays required and carries the message's id for a terminal recipient.
- `Pane`: optional `terminalHarness {id, name}` for a terminal pane whose foreground is a harness, and
  the open messages of its inbox (state, sender source, queued time, preview).
- One new event of the pane family, for the state of a terminal message (queued, delivering,
  delivered, needs_you, expired, dismissed, refused), with the sender source, the preview and the
  reason. Its dotted wire name is written out only when it lands in the contract, because
  `check_doc_names.py` reads every dotted name.
- `welcome.capabilities`: `pane-origin` (this engine classifies connections, so the person-only rule
  holds) and `terminal-messages` (typed and hook delivery work). A system that cannot classify
  offers neither, which turns the feature off there instead of leaving a hole open (below).
- `x-limits`: `terminalMessageBytes`, `terminalMessagesPending`, `terminalMessageTtlSec`,
  `terminalQuietMs`, `terminalSettleMs`, `terminalEchoMs`, `terminalSubmitMs`,
  `terminalMessagePreviewChars`.
- No new error code: `not_found` (not a harness, or not in the workspace), `pane_busy` (inbox full, a
  switch pending), `rate_limited`, `forbidden_role` (full access, or a program in a pane doing what
  only the person does), `platform_unsupported` (a system that cannot classify).
- Supervisor protocol 7: `Whose` and `Foreground` (not the engine's wire).
- The CLI: `soyeht message PANE TEXT`, `soyeht harness connect|disconnect|status`. `soyeht-mcp`'s
  pane-origin mode. Contract 08 section 2 (the table of who proves what: a connection with no binding
  is a human unless its origin is a pane) and 4, contract 04 R17 and a new contract 19 (the person-only
  table, the profiles, the safe-delivery rule and its limits) change with the code, in the same pull
  requests.

### Per system

| | macOS | Linux | Windows |
|---|---|---|---|
| Pane origin | audit token, session id and ancestry | `SO_PEERCRED`, `/proc`, subreaper where available | TCP row owner and the pane's job |
| Full-access rule | needs the signed app (an unsigned dev engine skips it) | the person-only rule only | the person-only rule only |
| Foreground program | `tcgetpgrp` and the process table | `tcgetpgrp` and `/proc` | newest leaf of the pane's job (a heuristic) |
| Terminal model and typing | one code for all three (the pty's bytes and the supervisor's input) | | ConPTY output is the same bytes; the profiles are tested on it |
| Harness configuration | the harness's own user config | the same | the same, paths from the harness |
| If a system cannot classify in time | n/a | n/a | the engine offers neither capability, answers a message to a terminal with `platform_unsupported`, and the person-only gap is listed in SECURITY.md as a known limit of that release |

The owner's daily use is macOS, so macOS lands complete first; Linux follows on the same code;
Windows has the most new work (the peer's process from a TCP port, the job test, the foreground
heuristic) and a fallback that is honest instead of silent.

### Tests

- **Unit.** The decision function is pure (inputs: the observation, the profile, the clock; output:
  wait, type, refuse with a reason) and is table-driven over `terminal-input-line.json`. The text
  cleaner (an end-of-paste marker, escapes, `\r`, bidirectional controls). The origin walk over a
  fake process table (a chain, a reused pid, a session, an orphan). Every state-changing method is in
  the person-only table or the pane table. The queue (order, cap, deadline, restart). `message_times`
  and the full-access check on both recipient kinds, beside `tasks/mode_cap/tests/message.rs`. The
  one `native_text` for both recipients. A test that `Role::is_person` has no production caller
  outside `Connected::person`.
- **Engine integration** (the runtime rig, `test-hooks`, never a production backdoor): a real
  supervisor and a small test harness program that draws an input box, enables bracketed paste, can
  open a dialog, an alternate screen, and exit. Hooks (`Hooks` gains the thresholds and a clock) make
  the timers short. Cases: delivery to an empty line; wait while text is in the line; wait during
  a keystroke; wait during a dialog and the alternate screen; the harness exits between check and
  write (no text reaches the shell, no Enter); a message with an end-of-paste marker; expiry and the
  notice to the sender; a restart with a message `delivering`; a connection from a process the test
  started inside a pane and one started outside it. An **attack test** (beside #168): a process
  started in a pane runs `soyeht pane prompt`, a raw `pane.input`, a raw `pane.permission.answer`
  and a task with full access against a full-access agent pane and gets `forbidden_role` each time,
  while `soyeht message` is delivered attributed or refused by #169; a `setsid` process on macOS with
  an attesting engine is refused the full-access prompt.
- **Real harness** (the `e2e` full tier, with `scripts/e2e_real_agent_messaging.py` as the model:
  `scripts/e2e_real_terminal_messaging.py`): a real Claude Code in a terminal pane, started by
  Soyeht and typed by the test, messaged by an agent pane (delivered, attributed; again with text
  half-typed in its line: it waits, then delivers once the line is cleared), then the terminal Claude
  sends to an agent pane and to another terminal through the Soyeht MCP, and a message to a pane in
  full access is refused. Codex, OpenCode, Cursor and Pi run the same cases, each with its own native channel and then with it
  disabled so the typed channel is proved too; Pi also proves the extension's two tools. All five
  are required for the capability of that harness: a harness that is not signed in is reported by
  name, as in the agent test, and fails the run, never skips it. A message that moves in a cross
  pair (a terminal Codex to a terminal Pi, an agent pane to a terminal OpenCode) is part of the matrix. The recorder runs through the
  collection scenarios above on the same rig.
- **Per system.** The Linux and Windows suites run the origin and foreground code on their systems
  (`scripts/check_engine_linux.sh`, `scripts/check_engine_windows.ps1`); a Windows run of the attack
  test is required before Windows reports the two capabilities.

## Slices

Sizes are an agent's working days: S 1 to 2, M 2 to 4, L 5 to 8. The first slice is the security
fix and lands alone, ahead of everything else.

| # | Slice | Size | Needs |
|---|---|---|---|
| 1a | **Pane origin and the person-only rule** on macOS and Linux: `soyeht-platform` process helpers, the supervisor's `Whose` (protocol 7), `Connected::origin` and `person()`, the person-only table and its contract test, the macOS full-access rule, the `is_person` callers, the CLI refusals and `soyeht message`, the attack tests, contract 08 and the first text of contract 19. Capability `pane-origin` | L | |
| 1b | The same on **Windows**: the peer from the TCP port, the job test; the desktop app's detach on Linux and Windows | L | 1a |
| 1c | The `binding_origin` and `origin_unknown` log events | S | 1a |
| 2 | **The sender**: `soyeht-mcp`'s pane-origin mode, the engine's grant of `agent.list` and `agent.message` to a terminal pane, `AgentMessageSource.kind`, `soyeht harness connect` and the per-launch way for Claude, `soyeht message`. A terminal Claude can message agent panes | M | 1a |
| 3 | **Foreground and listing**: the supervisor's `Foreground`, the harness table, `agent.list` with terminals, `Pane.terminalHarness`, full-access marks from arguments | M | 1a |
| 4a | **Observation and decision**: the engine's terminal model off the lock, the profile type and Claude's profile, the pure decision function, the text cleaner, the vectors from synthetic screens, the guarded paste in the supervisor | L | 3 |
| 4b | **Calibration**: the collection scenarios on real harnesses, the vectors from the recordings, the thresholds and the profiles of Codex, OpenCode and Cursor, held to the bar | L | 4a, the recorder |
| 5 | **The inbox and typed delivery**: the store table, the queue, the states, the deadline and the sender's notice, `agent.message` to a terminal, the event and the snapshot fields, the limits, the generated outputs, contracts 04, 08 and 19 | L | 3, 4a |
| 6 | **The native channel for Claude**: the hooks carry the inbox, always on, from the launch and from `soyeht harness connect`; the user-level hook with its 1.5 s ceiling | M | 2, 5 |
| 7 | **The apps**: the strip, Dismiss, the connect offer in Preferences, texts in `x-clientPolicy`, for the Mac, the desktop, the iPhone and iPad, and Android | L | 5 |
| 8a | **Codex and Cursor**: the foreground names, `soyeht harness connect` and the per-launch way (each by its own way in the table, tested with its real binary), the full-access signs, the native channel where it verifies (Codex hooks and `queue`, Cursor hooks) | M | 2, 4b, 5 |
| 8b | **OpenCode**: the same, with its plugin or local server API as the native channel | M | 2, 4b, 5 |
| 8c | **Pi**: the extension (`list_agents`, `message_agent`, the receiving side on `agent_settled`), `pi install` and the `-e` launch, the person-only "let agents message this pane" switch | M | 2, 5 |
| 9 | **End to end for all five**: `e2e_real_terminal_messaging.py` in the full tier with the cross-harness matrix, the per-system docs, `docs/platforms.md` | M | 5, 6, 8a-8c |

Order for the launch: 1a first and alone; then 2 and 3 (the owner's terminal Claude can send, and
terminals are listed); 4a with 4b started in parallel as soon as the recorder runs on a disposable
engine; 5; 6; 7 and 8a to 8c together (they are independent of each other); 9. If the date presses, the cuts in this order: the Windows
terminal-messaging capability (1b still lands, or the fallback in the table above is declared), the
native channels for any harness but Claude (each such harness keeps the typed channel and still sends and receives), the apps beyond the Mac and the desktop. The person-only rule
on a system that can classify is never cut.

## Alternatives considered

- **Count keystrokes, or know each editor's keys.** Rejected by the owner and by the Vim `dd` case.
- **Type whenever the input has been quiet.** The first manual test failed exactly this way. A pause is
  not an empty line, and people pause in the middle of a sentence.
- **A pane-scoped secret in the environment.** Everything the pane runs inherits it, and `ps` shows it.
  It protects nothing the pane origin does not, and leaks.
- **A shim of `claude` on the pane's `PATH`.** It changes what the person runs, breaks for an alias,
  an absolute path or a harness started from another tool, and is a surprise. A connection written to
  the harness's own configuration, with the person's consent, is visible and reversible.
- **Require the signed app for every person-only call.** Linux and Windows cannot attest an app, so
  it would end the desktop apps there.
- **Refuse at the command line only.** The socket is the same API with no command; a rule in the
  CLI is a rule for honest callers.
- **Treat a pane's mode as unknown and refuse.** It would end messaging to the daily Claude; the
  signs (arguments and screen) are what a reviewer can check, and the limit is stated.
- **Only Claude gets a native channel; the others type.** Rejected by the owner's scope: all five
  harnesses are first-class. Each gets the channel its own interface offers (table above), proved on
  the real binary, and the typed channel is the floor for all five.
- **One wrapper that hosts every harness behind a pty of its own** (so Soyeht controls the input
  line). It would make the harness a child of Soyeht's program, change how the person's terminal
  behaves, and replace five TUIs' behaviour with ours.

## What this does not stop

- A program of the same user that leaves a pane's process set on Linux or Windows is the person
  there (section 1). On macOS it is stopped only from the full-access prompt.
- The typed channel can be wrong only in the moment between the last check and the write, and then
  leaves one visible paste with no Enter; it cannot send what the person typed or what a shell
  would run.
- A harness in `tmux`, `screen`, `ssh` or a container is a plain terminal; a harness whose screen the
  profile cannot read is a plain terminal; a harness in full access that shows no sign is treated as
  not in full access (stated above).
- The native channel puts the text in the harness's context where the person may not read it at once;
  the strip is how the person sees it.
- A message is text another agent read somewhere. A harness that is not in full access still asks
  before it acts on it, which is the same protection #169 relies on for agent panes.

## Open questions

1. **Pi receiving agent messages.** Pi approves nothing, so by #169 an agent's message into it is
   refused. The owner asked that Pi receive; the design is a person-only per-pane switch, off by
   default, that lets agents message a terminal pane that approves everything. Is that the decision,
   or does Pi receive from the person only?
2. **A person who types `soyeht pane prompt` in a Soyeht pane** loses it, because the engine cannot
   tell what the person typed from what an agent ran in the same shell. They use the app, a terminal
   outside Soyeht, or `soyeht message`. Is that acceptable on every system? (On macOS a full-access
   pane is then the app's alone, even from a terminal outside Soyeht.)
3. **Windows.** If the peer's process from a TCP port, or the job test, is not ready for 0.1.0, is the
   fallback (the feature off, the gap listed) acceptable, or does the launch wait?
4. **Consent to write the harness's user configuration.** Is an explicit command and a Preferences
   switch per harness enough, or does the person see the exact change in the app first?
5. **A harness with no sign of full access** (OpenCode and Cursor may have none that shows on
   screen): treat as not in full access (this ADR), or refuse until its profile has a sign?
6. **The deadline** (15 minutes) and the sender's notice as a prompt in its pane. Is a notice into the
   sender's context the owner's wish, or only a mark in the app?
7. **A "send now" for a waiting message.** This ADR gives Dismiss only: sending is always the engine's
   decision from the screen. A person who wants it sent now can send it themselves.
8. **Native channels for Codex, OpenCode and Cursor** before launch, or after: each needs its mechanism
   verified on the real binary (table), and any that does not verify keeps the typed channel.
9. **Codex's `queue`** may reach a running interactive session through the shared local app-server; if
   it does, it is a better channel than typing for Codex. Verify before deciding.
