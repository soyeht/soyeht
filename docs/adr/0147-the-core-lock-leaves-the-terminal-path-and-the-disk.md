# ADR 0147: The Core lock leaves the terminal path, the disk and the idle wakeups

- Status: proposed
- Date: 2026-11-06

## Context

The engine keeps all of its state in one `Arc<Mutex<Core>>`. `Core` has 39 public fields and
its `impl` is spread over 93 files. Every request, every job result, the 10 ms tick and the
pump that sends terminal output to the clients take that lock (`core_lock.rs`). It is the
largest structural risk of the engine: anything that runs under it delays the terminal of
every pane on every connection, and the cost of a new feature under it is paid by everyone.

ADR 0079 made the rule "no external command, network request or directory walk under the
lock", and ADRs 0095, 0097, 0117, 0119 and 0126 moved the slow things off it one by one. Two
facts are not in the text people read first:

- The supervisor is **no longer** under the lock. ADR 0079's list of what still runs under it
  starts with "the terminal path to the supervisor socket, with 5 s timeouts", and that bullet
  is out of date. ADR 0117 moved the reads, the acknowledgements and `pane.input`, ADR 0119 the
  agent polls and every close, ADR 0126 the starts, the writes of agents, language servers and
  harness switches, and the last waits. `Core::pty_request` is gone. What is left under the lock
  on the terminal path is memory and the disk: the output is put into the pane's `Screen`
  (which writes its segment file), and the pump reads it back out of memory to the clients.
- What the lock still holds, by design, is: every SQLite read and write (a commit is an
  `fsync`: `synchronous=FULL`), bounded single-file reads and writes (editor open, the
  creation of writing documents), `file.search` with a replacement, `pane.experiment.export`,
  the segment-file appends of every terminal, and all the in-memory work of the handlers (parsing
  a document, building a snapshot). The only test that guards the lock is
  `tests/blocking_io_rule.rs`, and it checks external commands and the supervisor, not `std::fs`
  or the store.

A second cost of the single lock is not a stall but a wakeup. The tick runs every 10 ms, hops to
a blocking thread, takes the lock and runs the whole of `poll_output`, and `drain_terminal`
starts a supervisor `Drain` round trip for every pane on every tick, idle or not. The
performance audit of 2026-11-06 (release build) measured that as about 450 context switches a
second and 1.1% of a core with no pane at all, about 165 engine and 73 supervisor context
switches a second for each idle pane, and, for idle shells, 3.1% (engine) + 1.2% (supervisor)
of a core at 5 panes and 6.7% + 3.8% at 20 panes; the installed debug build costs 2 to 2.5
times that. `IDLE_PACE_MS` (`read_ahead.rs`) slows an idle **agent** to 100 ms; nothing slows a
shell. A prototype that stops reading a shell for 100 ms after an empty read brought 20 idle
panes from 10.5% to 2.1%, but it delays any output that nobody asked for (a prompt redraw after
a resize, a background job) by up to 100 ms, so it is not adopted.

## Measurement

Where the lock time goes was measured on a scripted session, not guessed. The measurement code
was local and is not part of this change: it records every hold of the lock by purpose (and by
method for `dispatch`), the time spent waiting for the lock, and, inside a hold, the time in
SQLite statements, in segment-file appends and reads, and in the editor's file port.

The session: one engine and one supervisor started by the integration rig, four terminal panes in
one workspace (one printing every 20 ms, one printing `seq 1 400000`, one printing 40 MB, one
typed into 150 times), four subscribed watchers, and a second client making 90 `file.open`,
`file.read`, `file.write` and `engine.status` requests each, three `big.txt` opens, three
`file.search`, one `file.search` with a replacement, `state.snapshot` and `workspace.create`. About
30 to 37 seconds. Debug build (the test engine), on a host whose load average was 35 to 125 because
of other work, so the **absolute** figures are inflated and noisy; the proportions are what is
reported. Seven runs; the two last, with the statement timer fixed, are the ones below.

| | Run A | Run B |
| --- | --- | --- |
| Session length | 36.7 s | 30.9 s |
| Total time the lock was held | 20.1 s (55%) | 11.6 s (37%) |
| of which SQLite statements | 19% | 26% |
| of which terminal segment-file appends and reads | 6% | 15% |
| of which the editor's file port | 0% | 1% |
| of which everything else (in-memory work) | 75% | 58% |
| `COMMIT` statements (the `fsync`) | 728, 1.81 s total, 2.5 ms mean, 44.8 ms max | 729, 2.10 s total, 2.9 ms mean, 38.5 ms max |
| Longest wait of the tick for the lock | 620 ms | 296 ms |
| Longest gap between frames of the 20 ms ticker pane, as seen by a watcher | 0.55 to 1.2 s over the runs | 0.52 s |

By purpose (Run A; holds in milliseconds):

| Purpose | Holds | Total | Mean | Longest | Notes |
| --- | --- | --- | --- | --- | --- |
| tick | 2398 | 7677 | 3.2 | 93 | SQL 1355, segment 480 |
| `file.open` | 93 | 5567 | 60 | 173 | SQL 442, file port 30; the rest is the editor building the document |
| `file.write` | 90 | 1863 | 21 | 107 | SQL 491 |
| background results | 582 | 1700 | 2.9 | 72 | segment 794 |
| `file.read` | 90 | 1150 | 13 | 120 | in memory |
| `file.search` | 4 | 628 | 157 | 627 | one with a replacement |
| `offload_apply` | 160 | 559 | 3.5 | 24 | SQL 383 |
| `pane.input` | 153 | 522 | 3.4 | 21 | SQL 363: two commits per keystroke |

What this says:

1. **The supervisor is not the problem any more, and the premise of the old list is wrong.**
   Nothing in the session waited for the supervisor under the lock. The stall that is left is
   made of the engine's own work.
2. **SQLite is the largest I/O item.** Commits are 60 to 70% of the SQL time. Every `pane.input`
   does two (the intent before the irreversible write, the receipt after it), so a keystroke
   costs about 2.4 ms of SQL in its dispatch hold and a second commit in the apply of its
   offload, about 5 ms of the global lock in this setup, and a typed character in one pane waits
   behind, and holds up, the output of every other pane. A stalled disk makes each of those 40 ms
   or more (ADR 0095 measured seconds on a busy Mac). Reads count too: one statement (the
   workspace by id) ran 10 012 times in a session.
3. **The terminal's own disk writes are under the lock.** Every output record is appended to
   the segment file with an open, a truncate, a seek, a write and a close, inside the tick or
   the result of a drain job (6 to 15% of the hold time). The live window is already in memory;
   the append to the file does not have to be.
4. **The editor's file port is small here (local disk) and unbounded in principle.** The same
   calls on a project in a network-mounted folder block for the latency of the mount, with the
   lock held. This is the case the owner described and it is not visible on a local SSD.
5. **Most of the time is plain computation** (`file.open`, `file.read`, `state.snapshot`).
   That is not blocking I/O, the rule of ADR 0079 does not cover it, and moving it does not need
   the same mechanism. The sharding of the terminal (step 3) is what keeps it away from output.
6. **The lock is busy 37 to 55% of the time in a busy session** (debug build, loaded host). Any
   hold of tens of milliseconds delays a terminal frame by tens of milliseconds; the existing
   `engine.core_lock_slow` log only sees holds of 100 ms or more and says nothing about why.

## Decision

Seven steps, each its own change with its own acceptance test, in this order. A step does not
start before the previous one has merged and its measurement is in the change. The first step
only makes the cost visible, so later steps are decided on the owner's machine, not on a
scripted session.

### Step 0. Say why a hold was slow

Behaviour unchanged. A hold, when it is logged as `engine.core_lock_slow`, also says how many
milliseconds of it were SQLite, segment files and file ports (`sqlMs`, `segmentMs`, `fileMs`),
and the guard also counts the time spent **waiting** for the lock and logs the longest wait
with the summary line. The causes are accumulated in a thread-local in `soyeht-platform` (the
store, the segment log and the editor port add to it with a monotonic clock around their calls;
the statement timer of SQLite has millisecond resolution and misses commits under 1 ms), and
the guard reads and clears it when the hold ends. The cost is a few nanoseconds per call and
the line is still written after the lock is released (`slow_holds.rs`).

- Risk: none to ordering. The accumulator is per thread, so a hold cannot be charged for work
  of another thread. Calls made off the lock add to a counter nobody reads; the guard clears it
  at the start of each hold.
- Test: unit tests with a store port and a file port that sleep, asserting the split in the line;
  `slow_holds.rs` keeps its own cases. The scripted session of this ADR becomes an ignored
  integration test (`tests/lock_profile.rs`) that prints the table above, so a later step shows
  its before and after on the same script.

### Step 1. Idle wakeups: the tick waits for events

The terminal path, the agents and the language servers all read supervisor processes, and all
poll them (`drain_terminal` on every tick; `take_read` in `read_ahead.rs`; the 100 ms idle pace).
The replacement is a wake from the supervisor, and a tick that sleeps until something is due.

**The supervisor says when a process has output.** A second connection to the supervisor, opened
by the engine after the request connection and using the same hello, carries a new request
`Watch` (the instances the engine cares about, with the offset it has read to). The supervisor
answers once and from then on **pushes** `Readable { instance_id, end_seq }` when the reader
thread of a process appends to its retention (it already knows the moment: it is the thread that
reads the pseudoterminal) and when a process exits. A notice is sent at most once until the
engine reads: the reader sets a per-instance "told" flag and the next `Drain` clears it, so a flood
of output makes one notice per read and not one per chunk. The connection is
a one-way stream after `Watch`, so it never takes the request connection's mutex
(`supervisor_handle.rs`) and a slow `Drain` does not delay a notice. The supervisor already serves
each connection on a thread of its own (`server.rs`), so this adds a thread per engine, not per
process. The supervisor's protocol version is bumped; an engine that meets a supervisor
that does not know `Watch` (an upgrade that left the old supervisor running) keeps polling, as
today, and logs it once.

**The engine reads on a notice, not on a tick.** A notice is turned into the wake the tick
already has (`Background::woken`, `spawn_prompt_background`): the notice thread marks the pane
as readable and notifies; the tick starts the drain job for that pane, whose result is applied
and pumped at once as it is today. A pane with nothing marked is not read. After a read that
found output, the pane is read again no sooner than one tick interval later, so a flood is
batched exactly as it is now, while the **first** read after quiet is immediate. Agents and
language servers use the same marks in place of `IDLE_PACE_MS` and the per-tick read that
`take_read` starts, and an agent's other timers (expiry, ADR 0080) are timers, below.

**A lost notice must cost latency, not output.** The first notice is not trusted alone:
registering a `Watch` returns, for each instance, whether it already has unread output (level
before edge), and the engine makes one sweep of all panes a second. A sweep that finds output no
notice announced increments a counter that is logged by kind; it should stay at zero, and a
test fails if it does not. A dropped events connection puts the engine back to polling at the
tick interval until it is made again, so the worst case is today's behaviour.

**The tick sleeps until the next deadline.** `poll_output` runs a dozen subsystems on every tick
only because it is periodic. Each of them (the editor's autosave and disk watch, the project tree,
the skills, the local-server scan at 5 s, the spawn and close retries, the harness switches, the
retired agents, the experiment captures, the task steps, the agents' timeouts, the minute prune)
is given a `next_due` and the tick waits on `woken` or the earliest of them, with a ceiling of
one second as a safety net and a floor of the tick interval while anything is active (an agent
turn, an open request, a replay). A subsystem whose work can only start from an event (a
request, a job result, a notice) does not need a deadline at all. With nothing active, the engine wakes
about once a second.

- Order and consistency: unchanged by design. The notice only decides **when** a read starts;
  the read, its offset (`screen.next_offset()` and `sequence_base`) and its application under
  the lock are the ones of ADR 0117, so a notice that arrives late, twice, or for a process that
  was replaced is harmless (the read is dropped as it is today when it no longer fits).
- Risks: (a) a missed notice leaves output unread until the sweep, one second; (b) a notice
  thread must never take the Core lock (it takes only the readable set's own mutex, which no one
  holds across a call), so there is no new lock order; (c) a subsystem with no `next_due` stalls
  silently, which is the failure of this step: every subsystem of `poll_output` must be listed
  with its deadline rule, and `tests/` has a test that runs the engine with the tick ceiling
  raised to a minute and checks each subsystem still does its work; (d) a flood of notices must not
  turn into a thread hop each, hence one notice per read.
- Keystroke echo: the path today is input, supervisor write, pseudoterminal echo, then a wait for
  the **next tick** (0 to 10 ms, 5 on average) and a `Drain`. It becomes input, write, echo,
  notice, `Drain`: the wait for the tick phase is gone. The VM1 editor and terminal latency meter
  is the acceptance check, on a quiet host (suspended VMs, no builds), main as the control, and the
  step does not merge if the echo p50 or p99 is worse than main's.
- CPU acceptance: the idle scenario of the audit at 0, 1, 5 and 20 panes, release build. Targets:
  no pane under 0.1% of a core for the engine; 20 idle shells at or under the prototype's 2.1% of a core
  in all (1.7% engine, 0.45% supervisor), with no added delay for unrequested output. The scenario is committed as
  an ignored bench beside `tests/idle_cpu.rs`.
- Not part of this step, and not to be mixed into it: the per-pane child-exit polling thread of
  the supervisor and the byte-by-byte copy in `retention.rs` are separate changes. They do not
  conflict: this step adds a push at the same place the reader appends (`start_readers`), and
  an exit notice can later replace the exit poll without changing the wire.

### Step 2. Per-pane terminal state in a shard the tick does not need Core for

This is the structural step. The terminal path (apply a drain result, append to the screen,
pump to the subscribers) becomes independent of the Core lock.

**What moves.** A `Terminals` hub owned by the engine next to `Core`, with its own mutex per
pane and one for the subscriber table:

- the pane's `Screen` (live window and segment log);
- the subscriptions: the cursor of each (connection, pane), and the handle of the connection's
  outbox, which is already a channel and does not need Core;
- a small **epoch** per pane that Core publishes whenever it changes under its own lock: the
  instance id, the sequence base, whether the pane is closed, whether it is an agent, whether its
  capture is waiting. A drain result is applied against the epoch exactly as
  `finish_terminal_drain` checks `saved` today.

**What stays in Core.** The pane's metadata and state (running, dead, exit code, phrase), every
store write, events, captures, agents. A drain apply that has news for Core (the process
exited, a capture wants the bytes) calls into Core through an ordinary background result, so the
rare path keeps its current code. Panes with an active capture (`terminal_captures`) are kept on
the existing path until a later step moves them; a pane is in one mode at a time, chosen by the
epoch.

**Lock order.** Core, then hub, then a pane's mutex; never the other way. The hub never calls
into Core and never blocks on anything but its own mutexes; the pump takes a pane's mutex only to
copy out the next records, then sends with it released. `pane.stream.subscribe` stays a Core
request (it needs the connection and the mapping frame to be ordered before any binary frame) and
takes the hub only to read the tail boundary and insert the cursor, atomically, so the boundary
and the first frame cannot disagree.

**What this buys.** Terminal output, the acknowledgement and the pump no longer wait for a
`file.open` that parses a document, a snapshot that serialises the workspace, or a store commit
that waits for the disk. They still wait on a pane's own mutex and, for the replay of old output,
on the disk (step 3 handles that).

- Risks: (a) **ordering** between a stream gap or replay-done notification and the durable events
  of the same pane: today both leave under one lock. Both go to the same per-connection outbox;
  the hub sends stream notifications and Core sends events, so the relative order of a stream
  notification and an event is no longer fixed. The wire never promised it (a client matches
  frames by stream id and offsets, events by `eventId`), but each client must be checked; the
  replay-done and gap frames stay ordered among themselves and with the binary frames of their
  stream, which is what the clients rely on. (b) **Deadlock** between the two locks: the order
  above, enforced by making the hub's methods the only way to reach a pane's mutex and by not
  handing a Core reference into them. (c) A result applied against a stale epoch: dropped, and
  the supervisor still holds the output (ADR 0117), as now. (d) A pane that closes while its mutex
  is held by the pump: close takes the hub's table lock first, marks the epoch closed, and the
  pump skips closed panes.
- Tests: `tests/core_lock.rs` gets a hook (`Hooks`, default off, implemented by `FileHooks`) that
  holds the Core lock for several seconds inside a request; the assertion is that frames of the
  ticker pane keep arriving at a watcher with a gap bounded by the control gap while Core is held.
  That test **fails today**, which is its point. A deterministic interleaving test (the pump and
  an epoch change, run under many schedules) covers the stale-epoch and close races; a
  subscribe-while-flooding test checks the first frame after the mapping starts exactly at the
  tail boundary.

### Step 3. The segment-file appends become write-behind

The live window is already in memory and `Screen::read` serves it from there. The disk is only
needed for output older than the window (a replay) and for crash recovery, and the supervisor keeps
the output until the flush job acknowledges it (ADR 0095). So the append to the file moves into the
flush job that already exists: the apply puts the record in the live window and in a queue; the
job writes the queue to the segment file, flushes, and acknowledges, off every lock.

- The segment log lives behind the pane's own mutex from step 2, so the job and the pump do not
  meet in Core. A replay that has to read the disk takes the pane's mutex for the read; a slow disk
  then delays the **subscribers of that one pane**, not the engine. To keep the pump from waiting on
  it, replays read the file through an offloaded job and send what they read, as `file.open` for a
  large file should (step 5).
- Risk: the window of records that are in memory and not yet on disk now includes records the
  engine has acknowledged to nobody (the acknowledgement still follows the flush), so a crash still
  loses nothing. The live window must not be trimmed (`take_in_through`) before the job has written
  what it trims; the order is enforced by the trim taking the position the job reports. A segment
  rotation or the removal of the oldest segment happens in the job, and the directory flush stays
  owed with the records, as it is.
- Tests: `tests/slow_disk.rs` gets a case with a slow segment **write** (the existing hook
  delays the flush only); a crash test kills the engine between the apply and the job and checks
  the output is drained again from the last durable position.

### Step 4. SQLite commits stop waiting for the disk under the lock

The commit is the one statement that waits for the disk, and it is done once per mutation, twice
per keystroke. Two ways were considered.

- **A store thread that owns the connection** and a handler that becomes "prepare under the lock,
  `Offload` the transaction, apply the result". It is the cleanest on paper and the most expensive:
  the 292 uses of `.store` or `StorePort::` in 86 files read and write in the same hold as the
  in-memory state they change, and each would have to be split into two phases with a re-check. It
  is rejected as a first move.
- **A durability barrier** (adopted). Transactions are written and committed under the lock with
  the WAL synced by a second step instead of in the commit: `synchronous=NORMAL` (the database
  stays consistent after a crash of the machine, only the last commits can be lost) plus an explicit
  sync of the WAL file, by a separate thread through its own file descriptor, that **coalesces**
  concurrent waiters into one sync (group commit). `Store::mutate_as` returns the transaction's
  `commit id` as before and now also a *horizon*: the commit sequence that must be synced before
  anything outside the engine can depend on it. Three things wait for the horizon, all in the same
  few places and none per handler:
  1. **Replies and events.** `Core::publish` queues events, and `Core::mutate` marks the hold as
     "owes a horizon". A handler's `Outcome::Reply` is turned, by `dispatch` itself, into an
     `Offload` that waits for the horizon and then answers, and the events are published in
     `eventId` order when it is reached. A client therefore never sees an event or a reply for state
     that a power loss could take back, which is the guarantee `domain.rs` states for `synchronous=FULL`
     today ("returning success is the durability boundary").
  2. **Irreversible effects.** `Offload::new` and `Core::spawn_background` capture the horizon at the
     moment they are created, under the lock, and their `work` starts by waiting for it. Every
     external effect the engine takes (the supervisor write of `pane.input`, a spawn, a git command, a
     file write) already goes through one of the two, which is why the intent of `pane.input` is
     durable before its write without any change in `input.rs`.
  3. **The tick.** Background results and the tick publish through the same queue.
- Risks, in the order they matter. (a) **Event order and replica consistency**: `eventId` is the
  `rowid` assigned inside the transaction and events are published after it commits; delaying
  publication must keep the ids in order, so there is one publish queue, drained by the horizon in
  commit order, and the snapshot and any reader of events after a cursor must not read an event the queue has not
  published (the cursor they return stays at the published horizon). A snapshot taken between a commit
  and its publication is the case to test. (b) **Reply visible before it is durable by another
  path**: a read-only request that sees an unsynced change (the state is in memory) must not be
  answered before the horizon either; the rule is "a hold that read state changed since the last
  synced horizon waits for it", which is cheap to state (the hold knows its read set is Core) and
  conservative. (c) **Checkpoints**: a WAL checkpoint syncs the database file; it runs inside a
  commit when the WAL passes `wal_autocheckpoint` pages, so the occasional long commit is still
  under the lock. It moves to the barrier thread (`wal_autocheckpoint=0` and an explicit passive
  checkpoint there). (d) **Idempotency receipts** are written in the same transaction as the change
  and cleared with it, so a retry after a crash finds both or neither, as now. (e) **A failed sync**
  must fail every waiter and put the engine in a state that refuses new mutations, because the
  replies it already queued cannot be sent honestly; this is the same class as a failed commit today.
  (f) The platform's `fsync` is not a disk flush on macOS unless `F_FULLFSYNC` is used; whatever the
  store uses today (`rusqlite`'s default) is kept, and the barrier uses the same call so the guarantee
  does not weaken silently.
- This step is **conditional**: it starts only if Step 0 shows `sqlMs` is a leading cause of slow
  holds in the owner's real engine log, and after the cheaper reductions below. If the hold times are
  dominated by in-memory work, the lock sharding of step 2 is enough and this step is not done.
- Cheaper reductions that come first, and may be enough: remove the second commit of a keystroke by
  recording the receipt in the same transaction that the next input starts, or batching it through the
  existing `send_later` queue; cache the point read that ran 10 012 times; look at why a language
  server checkpoint was written 277 times in the session.
- Tests: a store hook (the `test-support` feature of `soyeht-store`, like the segment flush pause)
  that delays the barrier's sync by seconds; the assertion is that terminal output and unrelated
  requests proceed (step 2 makes this true for output, this step for requests) while a mutation's
  reply waits. A crash test kills the engine process (not the machine) between commit and sync and
  checks that no reply was sent for the lost commit; a simulated sync failure checks the refusal; an
  event-order test runs mutations from several connections and checks the clients see strictly
  increasing `eventId`s with no gap, including across a snapshot.

### Step 5. Bounded file reads and writes use the same two shapes

The remaining work under the lock that touches a user's folder is moved to `Outcome::Offload`
(requests) and `Core::spawn_background` (engine work), by the rule of ADR 0079 (copy the inputs,
work with no lock, apply under the lock after checking again):

- **`file.open`**: the read of the text, the size and the encoding checks run in the work; the apply
  checks that the pane and the revision it started from are unchanged, then builds the document. The
  document build is computation and stays in the apply unless step 0 shows it is the larger part; if
  it is, it moves to the work too, since it needs no Core state.
- **Creation of writing documents** (`create_empty_text`, `create_text_if_absent`, with their
  `fsync`s): the work creates the file; the apply registers it.
- **`file.search` with a replacement**: today it plans and writes under the lock "so the editor
  buffers, the disk and the replacement journal stay consistent". It becomes a job that takes a
  journal first: the apply under the lock writes the journal and marks each file's buffer
  `replacing`; the job writes the files; the apply under the lock records the outcome. A request
  that edits a file marked `replacing` is refused as a conflict, the answer an edit already gets when
  its base revision is stale, so no new wire value is needed.
- **`pane.experiment.export`**: the export is built and written in the work from the copied
  selection; the apply records the receipt. It is rare, and the simplest of the four.
- Risks: the only one that matters is the consistency of the editor buffers with the disk during a
  replacement (above); the others re-check the same state a stale `Offload` re-checks today.
- Tests: the existing `editor_disk` hook gives a slow file port; `tests/slow_disk.rs` and
  `tests/core_lock.rs` each get one case per moved operation, asserting terminal output and
  `engine.status` keep their latency while the port stalls for seconds. The replacement gets a
  crash test over its journal (kill during the writes, restart, the journal finishes or reverts).

### Step 6. Enforcement

The rule test grows with the work, so a regression is a red test and not a slow afternoon.

1. **Ratchet on file I/O under the lock.** `tests/blocking_io_rule.rs` already holds a table of
   files that still wait for the supervisor and fails on a new one. It gets the same table for
   `std::fs` and `File`/`OpenOptions` calls in engine production code that run on the lock
   (the calls in `editor/`, `writing/`, `experiment_export.rs`, `screen.rs`, `agents/`, `skills/`),
   each file with its count and a reason, failing on a new file, on a count that grew, and on a count
   that fell without the list following. Files that only run off the lock (the work of an `Offload`,
   a background job, `soyeht-platform`) are exempt by being in a list of off-lock modules, which is
   itself reviewed. The list is the answer to "what still touches the disk under the lock" and each
   step above deletes lines from it.
2. **Ratchet on store use.** The same for `.store` and `StorePort::` in `soyeht-engine`: the 292
   uses in 86 files are counted, may not grow, and a reduction lowers the number. `rusqlite` is
   already only in `soyeht-store` (`hardening_gate.py check-no-sql-outside-store`), so this
   count is the guard on how much SQL the lock can hold.
3. **Runtime marks.** After steps 2, 3 and 5, the thread of a hold carries the mark that
   `soyeht_process::forbid` sets; the segment log, `soyeht_platform::fs` writers and the editor
   file port refuse to run on a marked thread (`Error::UnderLock`, as `run` does), and the few
   operations that must remain (a settings file, a registry file) take an explicit, named
   `allow_under_lock(reason)` that the test lists. Until then these calls only count (step 0).
4. **The lock's own invariant.** `tests/core_lock.rs` keeps its two cases and gains the held-Core
   case of step 2 and the slow-port cases of steps 4 and 5.

## What is not done, and why

- **A Core actor** (one thread owns `Core`, everything else sends messages). It would remove the
  lock and every order problem by construction, and it is a rewrite of 93 files whose handlers call
  one another with `&mut Core`. The shard and the barrier take away the two things that make the lock
  dangerous, the terminal and the disk, and leave the rest alone.
- **Several domain locks** (tasks, agents, panes, editor). The handlers cross those domains
  constantly (a task dispatch starts a pane that an agent owns and writes events), so the lock order
  would be a matrix of 5 to 8 locks with cycles that only appear under rare interleavings. The terminal
  is the one domain with a clean interface (output in, frames out) and the one whose latency is felt.
- **Pausing quiet shells** (the idle prototype). It trades idle CPU for up to 100 ms of delay on any
  output nobody asked for. Step 1 gets the same CPU with no delay.
- **`synchronous=NORMAL` alone**, with no barrier. It would remove the stall and also the guarantee
  that a reply or event is durable; a client could hold an `eventId` the database loses after a
  power failure and see another event reuse it.

## Consequences

- ADR 0079's list of what still runs under the lock is replaced by this record's: the supervisor
  was removed from it by ADRs 0117, 0119 and 0126. AGENTS.md "The Core lock" is updated in the first
  change that moves something, to point here.
- After step 2 the terminal's latency no longer depends on the cost of any request. After step 1 a
  machine with open panes and nothing happening is idle. After steps 4 and 5 a stalled disk or a
  stalled mount delays the request that asked, and nothing else.
- Each step is a change of its own that can be reverted alone.
- The ordering of a stream notification against a durable event for the same pane stops being a
  property of the lock (step 2). Clients do not depend on it; this ADR lists it so a client change
  does not start to.
