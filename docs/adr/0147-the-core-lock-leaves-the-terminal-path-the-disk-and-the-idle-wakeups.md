# ADR 0147: The Core lock leaves the terminal path, the disk and the idle wakeups

- Status: proposed
- Date: 2026-11-06

## Context

The engine keeps all of its state in one `Arc<Mutex<Core>>`. `Core` has 39 fields, 22 of
them `pub`, and its `impl` is spread over about 90 files. Every request, every job result, the 10 ms tick and the
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
   the same mechanism. The sharding of the terminal (step 2) is what keeps it away from output.
6. **The lock is busy 37 to 55% of the time in a busy session** (debug build, loaded host). Any
   hold of tens of milliseconds delays a terminal frame by tens of milliseconds; the existing
   `engine.core_lock_slow` log only sees holds of 100 ms or more and says nothing about why. The
   cost the owner feels sits mostly in holds of 3 to 60 ms (the means above), far under that
   threshold, so step 0 must measure every hold and not only the slow ones.

## Decision

The work is a sequence of changes, one pull request each. A change does not start before the one
it depends on has merged, and its measurement is in its description. The first change only makes
the cost visible, so the later ones are decided on the owner's machine and not on a scripted
session.

Order: **0**, **1a**, **R** (the cheap reductions), **5** (with the first ratchet of step 6),
**2a**, **2b**, **2c**, **1b**, **6**. Step 3 is the persistent segment handle. Write-behind (3b)
and step 4 are conditional on the data of step 0 and are not scheduled.

### Step 0. Say why a hold was slow, for every hold

Behaviour unchanged. The guard of the lock measures every hold, not only the ones past 100 ms:

- **Per hold:** how long it was held, how long its caller waited, and how the hold split into
  SQLite statements, the `COMMIT` among them, terminal segment-file reads and appends, and calls of
  the editor's file port. The wait is measured from the moment the request is made, so the queue of
  the blocking pool in `core_lock::with` is part of it, not only the mutex.
- **Where it is charged:** a thread-local in `soyeht-platform` (`hold_cost`) that the guard starts
  and finishes. There are no counters at the roughly 300 call sites of the store. Statements are
  timed at one place: the SQLite trace hook of the connection (statement start and finish, with a
  monotonic clock, since SQLite's own profile timer has a millisecond resolution and misses a commit
  under 1 ms), and the `COMMIT` statement is charged to its own column because it is the `fsync`
  of a transaction. How to read the two columns, which decides step 4: `sqlMs` is the lifetime of a
  statement, from its first step to its reset, so it includes the Rust code that runs between the
  steps (a row callback, a nested query) and statements that nest are summed; a write outside an
  explicit transaction commits inside its own `INSERT`, so that disk wait is in `sqlMs` and not in
  `commitMs`, and a `BEGIN IMMEDIATE` that waits for another writer is in `sqlMs` too. The disk
  wait of the engine is therefore `commitMs` plus the part of `sqlMs` that belongs to the writes
  outside a transaction; `commitMs` alone understates it.
  Segment-file reads and appends and the file port are charged where they are called (two functions
  of the segment log, the methods of the editor's port; a call inside another is charged once). Work
  done on a thread that holds no lock is not timed at all.
- **What is logged, always on:** the existing `engine.core_lock_slow` line gains the wait and the
  split (`waitedMs`, `sqlMs`, `commitMs`, `segmentMs`, `fileMs`) and a hold is also slow when the
  wait was 100 ms or more. And, once a minute, one usage line (`core_lock_usage`) for each of the ten
  busiest purposes (a request is a purpose of its own, named by its allow-listed method): the number
  of holds, the share of the minute the lock was held, the p50 and p99 of the hold and of the wait,
  the longest wait, and the sums of the five costs. A purpose that held the lock for less than
  5 thousandths of the minute is not written, so an idle engine, whose tick takes the lock a hundred
  times a second, does not write the same lines every minute. With this the question "does SQLite dominate
  the 3 to 20 ms holds" is answered from the owner's own log, which is what decides step 4.
- The trace hook is the `trace` feature of `rusqlite`, which is enabled only through `test-support`
  today (`soyeht-store/Cargo.toml`). Production needs it on in the normal build, so this change edits a
  `Cargo.toml` and is review-required for that reason too.
- Risk: none to ordering or consistency. Cost: two clock reads per hold and one per statement; the
  aggregate is updated after the lock is released.
- Tests: unit tests for the accumulator (nesting, no hold, bounded statement stack), for the
  percentile buckets and the top-ten selection, and for the line formats (`slow_holds.rs` keeps its
  cases); an engine test with a store port that sleeps, asserting the split on a slow hold. The
  scripted session of this ADR becomes an ignored integration test that prints the table, so every
  later change shows its before and after on the same script.

### Step 1a. The supervisor says when a process has output (the 10 ms tick stays)

Today every tick starts a supervisor `Drain` round trip for every pane, idle or not, and an idle
agent is read every 100 ms (`IDLE_PACE_MS`). This change replaces the **reads** with a push. The
tick, its period and everything else in `poll_output` stay as they are; step 1b is what changes the
tick.

**A second connection with a role.** The supervisor refuses a second connection of the same engine
generation (`ENGINE_CONNECTED`), a newer generation evicts the active link, and every request is
checked against the one active connection. So a second connection that says the same hello cannot
work. `Hello` gets an optional `role`, `#[serde(default)]`, whose default is the request role of
today. A **watch** role:

- is bound to the engine generation the supervisor has recorded and must **equal** it: an older one
  is refused as stale, and a newer one is refused too, since a watch can only follow the request
  connection of its own generation. A watch hello never writes the registry (only a request hello of
  a newer generation does);
- skips the "active connection" bookkeeping, so it neither evicts nor is evicted by the request
  connection, and is not counted by `ENGINE_CONNECTED`;
- is closed by the supervisor when a request connection of a newer generation arrives, together
  with the one it evicts;
- is made again, with a new `Watch`, whenever the engine's request link is replaced (a reconnect).
  The answer to `Watch` lists the instances that already have unread output (level before edge), so
  what was missed between the two links is read.

**No change of `wire::VERSION`.** A version mismatch fails the hello, and a supervisor must survive
an update of the engine (it holds the terminals, ADR 0123): a new version would either end the
person's terminals or leave the new engine unable to connect. The change is additive:

- `Welcome` gets a `features` list (`#[serde(default)]`), and the supervisor lists `watch` in it.
- **New engine, old supervisor:** the old supervisor ignores an unknown `role`, so the second hello
  is answered `ENGINE_CONNECTED` and refused. Also `features` is absent from its welcome. Either one
  means "no watch": the engine keeps polling exactly as today and logs it once.
- **Old engine, new supervisor:** the engine sends no `role` and no `Watch`; nothing changes for it.
- A test in the style of `tests/upgrade_recovery.rs` runs a new engine against the supervisor binary
  of the previous release (kept as a fixture of the test, built from the tagged source or vendored as
  a protocol stub that speaks the old hello) and checks that the engine works, polls, and logs the
  missing feature once. The reverse pair (previous engine, new supervisor) is a second case.

**What is pushed.** After the hello of the watch role, the engine sends `Watch` with the instances
it cares about and, for each, the offset it has read to. The supervisor answers with the instances
that already have unread output (level before edge), and from then on pushes
`Readable { instance_id, end_seq }` when a process has output past what the engine last read, and
when a process has exited.

- The pseudoterminal reader thread of the supervisor **never writes to the socket**. It marks the
  instance in a dirty set and wakes one writer thread per watch connection; the writer owns the
  socket, so the 5 s write timeout of a slow engine cannot stall a shell.
- The "told" flag is cleared in the same critical section of the output mutex that the `Drain` read
  takes, so a chunk that arrives between a read and the clearing cannot be lost: either the read has
  it or the flag is still set and the next notice follows.
- An exit is announced only after both `exited` and `read_done` are set, so the engine never reads
  an exit before the last byte of output.
- A flood makes one notice per read, not one per chunk.

**The engine reads on a notice.** The notice thread takes no Core lock. It marks the pane as readable
in a small set with its own mutex and sets the wake the tick already has (`Background::woken`). The
tick starts the drain job for each marked pane. Two rules, and they do not overlap:

- A read that was **truncated** (`Drain` returns at most 64 KiB, and `end_seq` is past the offset it
  reached) is repeated at once, with no wait for a tick, until it reaches `end_seq` or has been
  repeated 16 times in a row, after which the pane waits for the next tick like the other panes. A
  flood is thus limited by the round trip and no longer by the tick, whose 64 KiB per 10 ms is today's
  ceiling of about 6.4 MB/s per pane; the 16-repeat cap keeps one flooding pane from taking every pool
  worker and the lock from the others. The cost is more lock occupancy under a flood, which step 0's
  data and the meter show.
- A read that **reached** `end_seq` clears the mark. If it found output, the pane is not read again
  sooner than one tick interval, which batches many small writes the way they are batched now; the
  first read after quiet is immediate. The
agents and the language servers use the same marks in place of the per-tick read of `take_read` and
of `IDLE_PACE_MS`.

**A lost notice costs latency, not output.** The engine sweeps every pane once a second and counts a
miss when a sweep finds output for a pane that has no mark, no job in flight and no notice during the
previous two sweeps. The miss is logged by kind and a test fails when it is not zero. A dropped watch
connection returns the engine to polling at the tick interval until it is made again.

- Order and consistency: unchanged by design. A notice only decides when a read starts; the read,
  its offset and its application are those of ADR 0117, so a notice that is late, doubled or for a
  replaced process is harmless.
- Risk: a second connection for the supervisor to serve (a thread per watch, not per process); a
  missed notice (bounded by the sweep); a watch writer that blocks (its own thread).
- Keystroke echo today is input, supervisor write, echo, a wait for the next tick (0 to 10 ms) and a
  `Drain`. It becomes input, write, echo, notice, `Drain`. The VM1 editor and terminal latency meter
  is the acceptance check, run by the owner on a quiet host with main as the control; a scripted
  harness measures keystroke-to-echo before and after in the change. The change does not merge if
  the echo is worse than main's.
- Acceptance of the idle cost is by **counts**, which do not depend on the host: supervisor requests
  per idle second per pane (today about 100 for a shell), engine wakeups per second, and context
  switches, at 0, 1, 5 and 20 panes, release and debug. The CPU percentages of the audit stay an
  ignored bench. Target: an idle shell makes no supervisor request except the sweep.
- Not part of this change: the per-pane child-exit polling thread of the supervisor and the
  byte-by-byte copy in `retention.rs` are separate work by another change. This change adds a push
  where the reader appends (`start_readers`); an exit notice can later replace the exit poll without
  a wire change.

**As built (change 1a).** The wire is as above, with these details and two narrowings:

- A process the engine does not name in `Watch` counts as read to offset 0, so the engine names
  none and the answer is every process with output and every process that exited: one pass over
  all of them when a watch starts, which is also the first sweep.
- A read clears a mark only when it reached the larger of what the notice announced and what the
  supervisor held when the read was made (`Output.end_seq`); a notice is sent once per read, so the
  first one of a burst says little. A read that found output and reached the end leaves the process
  to the regular tick for one tick (the 10 ms `PACE`), so a process that writes in small chunks
  costs a read and an apply per tick and not one per round trip; a truncated read is repeated at
  once.
- A watch notice is a mark with the end it announced. A read clears it when it reached that end (or,
  for a mark that a sweep made, always); a notice that arrives during a read raises the end, so
  that read leaves the mark and the process is read again. A pane whose reads keep truncating is
  repeated at once by a woken tick up to 16 times in a row; the regular tick always reads a marked
  pane.
- **Agents** keep their poll and their pause: it cannot advance without a read, and the native
  records, deadlines and the expiry of requests of an idle agent must not be late. An idle agent
  is still read every `IDLE_PACE_MS` (100 ms); a mark ends the pause at once, so output is not
  waiting for it.
- **Language servers are not moved in this change.** They are read on every tick, as before: they
  exist only while an editor has a source file open and their poll needs a read each tick. They move
  with step 1b, where the tick has deadlines.
- Counts, in the release build on a loaded host (load average 80 to 130, so the CPU and latency
  figures are only a direction): context switches per second of the engine and of the supervisor
  fell from 806 and 138 (1 shell) to 499 and 36, from 1739 and 689 (6 shells) to 561 and 125, and
  from 2339 and 1671 (21 shells) to 572 and 693. What is left in the engine is the 10 ms tick (step
  1b); what is left in the supervisor at 21 shells is the 10 ms exit poll of each process, which
  another change replaces. The echo of a typed key, measured from the send of the input to the
  frame that carries the echo, was at the median 31 ms before and 21 ms after with 1 shell, 64 and
  57 ms with 6 shells, and 63 and 63 ms with 21; p99 fell in each case. The meter on a quiet host
  decides.

### Step R. The reductions that need no new mechanism and no change of durability

- The second commit of a keystroke. `pane.input` records the intent before the irreversible write
  and the receipt after it (two commits). The receipt only lets a retry with the same key be
  answered from the database; the supervisor already deduplicates the write by its operation key
  (ADR 0117). If the receipt can be recorded with the next durable write of the same connection, or
  through the `send_later` queue, without a reply being sent before the intent is durable, the
  commit count of a keystroke halves and nothing weaker is promised. If it cannot be shown to keep
  the guarantee, it is not done.
- The workspace read that ran 10 012 times in the session: find the callers, and read it once per
  hold, or keep the row in `Core` (the writer is `Core`).
- The language-server checkpoint that was written 277 times in the session: find out why, and write
  it when it changes.
- Test: the statement counter of `soyeht_store::test_support` (`statements_started`) gets a case per
  path, asserting the counts fall and no reply precedes the intent.

### Step 2. The terminal path no longer needs the Core lock

This is the structural step. It is three changes, because one is too large to review.

**What the hub is.** A `Terminals` hub owned by the engine beside `Core`. It owns, per pane, an
entry holding the `Screen`; a table of the subscriptions (the cursor of each connection and pane and
the handle of the connection's outbox, which is a channel and needs no Core); and the **scheduling**
of the pane's reads, flushes and acknowledgements: the in-flight flags, a clone of the `Supervisor`
handle with the link generation it belongs to, and the jobs themselves. The flags matter: today the
set of jobs in flight is cleared only when `Core` applies a finished job, so with Core held no read
and no acknowledgement can start, frames stop, and a flood overflows the 8 MiB the supervisor keeps
per process and produces declared gaps. A pane's drain, flush and ack therefore run entirely in the
hub. Core learns only what concerns it (the link failed, the process exited, a capture wants the
bytes) through an ordinary background result.

**Lock order, and two rules that keep the hub from becoming the new bottleneck.**
Core, then the hub's table, then a pane's mutex; never the other way, and the hub never calls into
Core.

1. A pane's mutex is **not held across disk I/O, with one exception: the append.** `Log::read` can
   open up to 256 files; the reader copies what it needs (the index entries and the file numbers)
   under the mutex, releases it, reads, and takes it again only to hand over the result. The flush
   runs on a copy of the file list, as it does today. The append of a record (and the rotation and the
   `remove_file` of the oldest segment it can cause) stays under the mutex: it writes to the page
   cache and never calls `fsync` there (ADR 0095), so it is short and bounded, and doing it under the
   mutex is what keeps the file and the window in step. Core never waits for that mutex on a hot path,
   because what it reads are the atomics of rule 2; only the hub's own threads and the readers of
   records wait for it.
2. **Core handlers do not wait for a pane's mutex.** `state.snapshot` reads every pane's offsets, and
   about 50 other sites read `.screen` (`core.rs`, `agents/lifecycle.rs`, `agents/switching/`,
   `agents/driver.rs`, `tasks/dispatching.rs`, and more). The hub entry publishes `first`, `next` and
   `tail_start` as atomics, written under the pane's mutex after each change, and those readers use
   them. A reader that needs the records takes the mutex (rule 1 keeps that short).
3. A panic while a pane's mutex is held poisons that pane only; the hub's policy is that the pane
   reads as failed and the rest of the engine, `state.snapshot` included, goes on.

**Epoch.** Core publishes to the hub, per pane, what the apply needs to decide whether a read still
fits: the instance id, the sequence base, `closed`, whether the pane is an agent or has a capture
waiting to be restored, and what the pump needs (`stream_id`, `generation`, `mode`, `recording`).
Not all of it lives in `stored`: "is an agent" is `Core.agents` and "capture waiting" is
`capture_restore`, and each changes at about a dozen places (`agents.insert` and `remove`, and the
edits of `capture_restore`). `RunningPane.stored` is assigned directly at about a dozen more
(`core.rs`, `panes.rs`, `supervisor_link.rs`, `web/`, `agents/`, `editor/project_entries.rs`,
`experiment_build.rs`). In step 2a all three go through one publisher: `stored` becomes private behind
a setter that republishes the epoch, and the insertions and removals in `agents` and the edits of
`capture_restore` are methods that republish it. A test build checks after every hold that the
published epoch equals `f(stored, agents, capture_restore)` for every pane.

**2a. Types.** `Screen` and the subscriptions move into hub types and the offsets become atomics.
Core still drives the reads and the pump, exactly as now. Reviewable on its own: no behaviour changes,
the lock order and the setter are in place, and the 50 call sites read the atomics.

**2b. Apply and pump in the hub.** The application of a finished read (the check against the epoch,
the append, the trim) and the pump of the subscribers run in the hub.

- **Single pumper:** a subscription is pumped by one thread at a time (a token on the subscription,
  or one pump per connection), so frames of a stream never interleave.
- **A paced replay driver.** A subscription that replays old output sends in pages within the
  outbox's byte budget (8 MiB per connection). Without the 10 ms tick to call it again, a pump that
  stopped because the budget was full is woken by the **connection's writer**: when the writer drains
  the queue below the budget it signals a per-connection notify, and the pump of every subscription
  that is behind re-arms on it. A short timer (50 ms) runs while any subscription is behind, as a
  safety net for a signal that was missed, so a replay always finishes.
- **Subscribe:** `pane.stream.subscribe` stays a Core request. In one hub critical section it
  captures the tail boundary, enqueues the mapping response and the replay start, and inserts the
  cursor. Outbox sends do not block, so sending under the hub's lock is safe, and no binary frame can
  be pumped before the mapping.
- Ordering. A stream notification and a durable event of one pane do not share a lock any more. They
  were never ordered by the lock alone: the outbox already sends control frames before terminal frames
  by priority. The clients match frames by stream id and offsets and events by `eventId`; each client
  is checked, and the replay-done and gap frames stay ordered with the binary frames of their stream.

**2c. The read, flush and acknowledgement loop in the hub, and the held-Core test.** The loop of
section "What the hub is" moves; the test below proves it. Without a working watch (an old
supervisor, or a dropped watch connection) the engine polls, and after this change the poll cannot
be the Core tick, which is blocked in the held-Core test and whenever a handler is slow: the hub owns
a **poll timer** of its own that marks every pane readable once per tick interval while there is no
watch, and the same timer carries the one-second sweep of step 1a. The held-Core test runs in both
modes.

- Risks of the whole step: (a) a deadlock between the two locks, prevented by the order, by the hub
  being the only way to reach a pane's mutex and by never passing a Core reference into it; (b) a
  result applied against a stale epoch is dropped and the supervisor still holds the output (ADR
  0117); (c) a pane that closes while its mutex is held: close takes the table lock first, marks the
  epoch closed, and the pump skips closed panes; (d) capture panes stay on the existing path until a
  later change, selected by the epoch.
- Tests:
  - **The held-Core test.** A hook (`Hooks`, default off, implemented by `FileHooks`) holds the Core
    lock for several seconds inside a request. The hold is proved by the latency of `engine.status` on
    another connection. A ticker pane prints numbered lines, and the assertion is that the lines
    delivered during the hold were produced during the hold (not a replay of earlier ones), with an
    absolute bound of about 250 ms on the gap between frames. Run with and without Watch (step 1a), and
    on Windows too (`core_lock.rs` is unix-only today; the hook and the ticker are written without a
    shell dependency). It fails today.
  - The interleavings are tested on a pure hub type (no engine) under a model checker or with yield
    hooks: stale epoch against close, pump against replay, subscribe against a flood. A subscribe while
    the pane floods checks that the first frame after the mapping starts at the tail boundary.

### Step 1b. The tick sleeps until the next deadline

After 1a nothing needs the tick for terminal reads. `poll_output` still runs a dozen subsystems on a
fixed period. Each of them (the editor's autosave and disk watch, the project tree, the skills, the
local-server scan at 5 s, the spawn and close retries, the harness switches, the retired agents, the
experiment captures, the task steps, the agents' timeouts, the minute prune) is given a `next_due`,
and the tick waits on `woken` or the earliest deadline, with a ceiling of one second as a safety net
and a floor of the tick interval while anything is active (an agent turn, an open request, a replay).
A subsystem whose work can only start from an event has no deadline. Idle, the engine wakes about once
a second.

- The risk of the step is a subsystem with no `next_due` that stalls silently. Every subsystem of
  `poll_output` is listed with its rule, and an integration test runs the engine with the ceiling at
  one minute and checks that each still does its work.
- Cheaper alternative inside the step, if the deadline rules are too large for one change: skip the
  tick's hop to a blocking thread and its lock when nothing is pending, tested by an atomic "pending"
  check that every producer of work sets.
- Acceptance: the counts of step 1a (ticks per second at 0 panes), and the meter as before.

**As built (change 1b).** The plan held, with these differences, which are the boundaries that moved:

- **A subsystem says when it needs the tick; it does not store a `next_due`.** After each run the
  tick calls `Core::tick_wait` (`tick_due.rs`), which asks each subsystem for its deadline in the
  engine clock and takes the earliest, never later than the ceiling. The deadline of a retry, an
  autosave or a burst of file events is data the subsystem already had (`due_ms`, `save_due_ms`,
  `Burst::due_ms`); a second field would be a second copy. The ceiling is `Hooks::tick_ceiling`
  (default one second), so a test can raise it.
- **The tick is raised by events, and the list of what produces work is not enumerated.** Every
  background job that ends, every hold of the Core lock that is not the tick's own (a request, an
  offload's apply, a connection closing) and every file event raises a `kick`; the tick then runs in
  full once the floor of one interval since its last start has passed. A request is how nearly every
  subsystem gets new work (a pane to spawn, a file to autosave, a task to dispatch), and there are
  about 300 places that do it, so the rule is that the lock raises the tick, not that each place says
  so. The 1a wake (`woken`) stays for what is worth applying at once (a read that found output, a
  notice), and also schedules a full run.
- **A run that wrote to the store runs again after the floor.** What a subsystem wrote may give
  another one work (a task reaches a state that a review waits for), and most of the task steps
  poll the store, so the chain of such steps moves at the floor and stops on the first run that
  writes nothing. The polls that rest (`Store::begin_poll`) report their own wake: `next_poll_wait`
  is zero for a poll that is not at rest and the end of the rest for one that is, so the loops, the
  retries of blocked tasks and the expiry of web permissions have their deadlines from the rows.
- **Agents at rest are read on a notice or the sweep, not on a pace.** With the watch heard, a
  settled agent (`Idle`) is not read every `IDLE_PACE_MS`: a mark of the watch ends the rest, as
  1a said, and so does a read that ended and waits to be taken. The pace stays for the engine
  without a watch. An agent that is not settled keeps the floor, and a settled one has the deadline
  of its first open request.
- **Language servers move to the marks** (1a left them). With the watch heard a server is read when
  it is marked, when a read ended, or when a write is pending; its process is part of the sweep
  now, so a notice that never came costs a second here too. Without the watch it is read on every
  run as before.
- **What keeps the floor, because nothing announces it:** a shell, an agent or a language server
  while no watch is heard (the polling of 1a), the harness switches, the experiment captures, the
  rest of a replay of a pane's history to a client (`streams_behind`: a pump carries one batch), and
  the task polls that are not at rest.
- **What the idle engine still wakes for:** the sweep and the ceiling (once a second each), the
  disk check of an open file the editor watches (250 ms for each, a read of the file; the project
  watch could report it by events), the skills probe (2 s), the scan of the local servers (5 s, one
  `lsof`), the usage schedule (15 s, a task of its own), and the supervisor's own exit poll, which is
  other work. The first of these that costs on a laptop with an editor open is the disk check.
- **Counts** (release build, the test engine, on a host whose load average was 12 to 26, three runs
  of each, main and this change one after the other): context switches a second of the engine fell
  from 468 to 499 with no pane and from 457 to 501 with five idle shells to 15 to 20 and 46 to 59;
  its CPU from 1.3 to 1.6% and 1.5 to 1.8% of a core to under 0.2% (`ps` has a resolution of 10 ms,
  so a window of six seconds cannot say less); the supervisor's 10 to 12 a second are its own exit
  poll and did not move. The median echo of a key typed into an idle shell, from the request to the
  frame that carries the echo, was 2.1 to 19.3 ms before and 1.1 to 6.4 ms after, the spread being
  the host's. The meter on a quiet host decides.
- **Tests.** `tick_due` has unit cases for the deadlines of each family and for the ceiling;
  `tests/idle_wakeups.rs` counts the context switches of the engine with no pane and with five
  idle shells and fails above 150 a second (main made 440 to 500 in either case), and fails if the
  median echo of a key in an idle shell takes 400 ms (the tick's ceiling would put it at a second);
  `tests/tick_deadlines.rs` runs an engine whose ceiling is a minute and checks, without sending
  a request (a request raises the tick and would hide a missing deadline), that a shell's output,
  the editor's autosave and disk watch, the project panel, the skills and the reconnection to a
  restarted supervisor still happen within seconds.

### Step 3. Persistent segment handle; write-behind only if the data asks for it

Every record appended to a pane's segment file opens the file, truncates it to the recorded size
(`set_len`), seeks, writes and closes it (`soyeht-store/src/segments.rs`). That is about five system
calls per record under the lock, 6 to 15% of the hold time in the session.

- **3a (scheduled).** The active segment's `File` stays open between records and the per-record
  `set_len` is dropped: a torn tail is already truncated when the log is opened (`Log::open`). The
  handle is closed on rotation and with the log. The per-record `set_len` also repairs a partial write
  made in this process (the disk was full in the middle of a record), so on **any** write error the
  handle is dropped, the file is reopened and truncated to the recorded size, and the record is not
  counted as written, as now. The syscall count per record falls to one. No change
  of where the write happens, so no change of ordering, trimming or backpressure. Test: a crash test
  that kills the engine in the middle of a record and checks `Log::open` truncates the tail and the
  output is drained again from the last durable position; the statement and syscall counts of a
  burst. A Windows case: an open handle must not block the rotation that removes the oldest segment
  (the active segment is never the one removed, and a log with a single segment closes its handle
  before it rotates) nor the deletion of a closed pane's folder (the handle is closed first, and the
  file is opened with delete sharing).
- **3b (conditional, not scheduled).** Moving the append into the flush job. It is not as simple as
  it looked: the live window trims inside the append (`TAIL_BYTES` or 1024 records), not at
  `take_in_through`, and `Screen::read` reports a range that is outside the window and past what is on
  disk as a **gap**, so data still waiting in a write queue would be reported missing. It would need a
  bounded queue, a trim of the window coupled to the writer's position, and backpressure that stops
  draining the supervisor when the queue is high. It is done only if, after 3a, the segment cost in
  the step 0 data is still a leading cause of slow holds.

### Step 4. A durability barrier for SQLite commits (conditional, not scheduled)

The commit is the one statement that waits for the disk, once per mutation. Two ways were considered.

- **A store thread that owns the connection** and handlers that prepare under the lock, offload the
  transaction and apply the result. The cleanest and the most expensive: the roughly 290 uses of
  `.store` or `StorePort::` in about 86 files read and write in the same hold as the in-memory state
  they change. Rejected as a first move.
- **A durability barrier.** Transactions are written and committed under the lock with
  `synchronous=NORMAL` (the database stays consistent after a crash of the machine; only the last
  commits may be lost) plus an explicit sync of the WAL file by another thread through its own
  descriptor, which coalesces waiters into one sync. `Store::mutate_as` also returns a *horizon*, and
  three things wait for it: replies and events (`dispatch` turns a reply into an `Offload` that
  waits, and events are published in `eventId` order when the horizon is reached); irreversible
  effects (`Offload::new` and `Core::spawn_background` capture the horizon when created, under the
  lock, and their work starts by waiting for it, which makes the intent of `pane.input` durable before
  its write with no change in `input.rs`); and the tick.
- Risks: (a) **event order and replica consistency**: `eventId` is the row id assigned in the
  transaction; publication is delayed but must stay in order, with one publish queue drained by the
  horizon, and a snapshot or a reader of events after a cursor must not see an event the queue has not
  published; (b) a read-only request that saw a change not yet synced must wait for the horizon too;
  (c) WAL checkpoints sync the database file and run inside a commit past `wal_autocheckpoint`; they
  move to the barrier thread; (d) a failed sync fails every waiter and the engine refuses new
  mutations, as a failed commit does; (e) the sync call must be the one the store uses today, so the
  guarantee is not weakened silently.
- It starts only if the data of step 0 shows SQL (and the `COMMIT` in particular) as a leading cause
  of the 3 to 20 ms holds, after step R.
- Tests: a store hook that delays the barrier's sync by seconds, asserting unrelated requests and
  terminal output proceed while a mutation's reply waits; a crash test between commit and sync
  (no reply for the lost commit); a simulated sync failure; an event-order test from several
  connections across a snapshot.

### Step 5. Bounded file reads and writes use the same two shapes

The remaining work under the lock that touches a user's folder moves to `Outcome::Offload` (requests)
and `Core::spawn_background` (engine work), by the rule of ADR 0079: copy the inputs, work with no
lock, apply under the lock after checking again.

- **`file.open`:** the read of the text and the checks run in the work; the apply checks that the pane
  and the revision it started from are unchanged and then builds the document. The build is computation
  and stays in the apply unless step 0 shows it is the larger part; then it moves to the work too, since
  it needs no Core state.
- **Creation of writing documents** (with the `fsync`s): the work creates the file; the apply registers it.
- **`file.search` with a replacement.** It plans and writes under the lock today "so the editor buffers,
  the disk and the replacement journal stay consistent". It becomes a job that takes the journal first:
  the apply under the lock writes the journal and marks each file `replacing`; the job writes the files;
  the apply records the outcome and clears the marks. While a file is `replacing`: an edit of it is
  refused as a conflict (the answer an edit with a stale base revision already gets, so no new wire
  value); **the disk watch and the autosave skip it**, so neither reads a half-written file as an
  external change nor writes the buffer over the replacement; and when the replacement finishes each
  file's buffer is reloaded from what was written, the watch is re-armed with the new disk state, and an
  edit made during the replacement (refused) is not lost, because it was never accepted.
- **`pane.experiment.export`:** built and written in the work from the copied selection; the apply
  records the receipt. Rare, and the simplest.
- With this change the first **ratchet** of step 6 lands (section below), so the list of what still
  touches the disk under the lock exists from the first move on and each move deletes lines from it.
- Tests: the `editor_disk` hook gives a slow file port; `tests/slow_disk.rs` and `tests/core_lock.rs`
  each get one case per moved operation, asserting terminal output and `engine.status` keep their
  latency while the port stalls for seconds. The replacement gets a crash test over its journal
  (killed during the writes; on restart the journal finishes or reverts).

### Step 6. Enforcement

The rule test grows with the work, so a regression is a red test.

1. **Ratchet on file I/O under the lock** (first in step 5). `tests/blocking_io_rule.rs` already holds a
   table of the files that wait for the supervisor and fails on a new one. It gets the same table for
   `std::fs`, `File` and `OpenOptions` calls in the engine's production code that can run on the lock
   (`editor/`, `writing/`, `experiment_export.rs`, `screen.rs`, `agents/`, `skills/`, ...), each file with
   its count and a reason, failing on a new file, on a count that grew and on one that fell without the
   list following. Modules that only run off the lock are in a list of their own, itself reviewed. Crates
   below the engine (`soyeht-editor`, `soyeht-writing`, `soyeht-skills`) have `std::fs` too; they are
   guarded only by this static ratchet, because the runtime mark below cannot see them.
2. **Ratchet on store use.** The same for `.store` and `StorePort::` in `soyeht-engine`: the roughly 290
   uses may not grow, and a reduction lowers the number. `rusqlite` is already only in `soyeht-store`
   (`hardening_gate.py check-no-sql-outside-store`).
3. **Runtime marks.** The thread-local that marks a thread as holding the lock belongs in
   `soyeht-platform` (where `hold_cost` already lives), and `soyeht-process` re-exports it, so the
   segment log, the writers of `soyeht_platform::fs` and the editor's port can refuse to run on a marked
   thread (`Error::UnderLock`, as `run` does). The few operations that must stay under the lock (a
   settings file, a registry file) take a named `allow_under_lock(reason)` that the test lists. Until the
   moves are done these calls only count (step 0).
4. `tests/core_lock.rs` keeps its cases and gains the held-Core case of step 2c and the slow-port cases
   of steps 4 and 5.

## Alternatives considered

- **A Core actor** (one thread owns `Core`, everything else sends messages). It removes the lock and its
  order problems by construction, and it is a rewrite of about 90 files whose handlers call one another
  with `&mut Core`. The hub and the barrier take away what makes the lock dangerous (the terminal and
  the disk) and leave the rest alone.
- **Several domain locks** (tasks, agents, panes, editor). The handlers cross those domains constantly (a
  task dispatch starts a pane an agent owns and writes events), so the order would be a matrix of 5 to 8
  locks whose cycles appear only under rare interleavings. The terminal is the one domain with a clean
  interface (output in, frames out) and the one whose latency is felt.
- **Pausing quiet shells** (the idle prototype). It trades idle CPU for up to 100 ms of delay on output
  nobody asked for (a prompt redraw after a resize, a background job). Step 1a gets the same CPU with no
  delay.
- **A long poll on the request connection.** A request that waits in the supervisor blocks the one
  stream of requests and answers (ADR 0117), so every other call would queue behind it. The watch
  connection is separate for that reason.
- **Fallbacks for step 1a**, kept in case the supervisor change is refused or delayed:
  1. a batched poll: one request carrying `(instance, from_seq)` for every pane and returning only those
     with output, which turns N round trips per tick into one;
  2. a back-off for shells that have been quiet for more than a second, reset by an input or a resize
     (the prompt redraw after a resize is then not delayed, because the resize resets it); background
     output of a quiet shell is still delayed by the back-off, which is why it is a fallback;
  3. skipping the tick's hop to a blocking thread and its lock when nothing is due (an atomic pending
     check), which removes the lock acquisitions of an idle engine but not the supervisor round trips.
- **`synchronous=NORMAL` alone**, without a barrier. It removes the stall and also the guarantee that a
  reply or an event is durable: a client could hold an `eventId` the database loses after a power
  failure, and another event would then reuse it.

## Consequences

- ADR 0079's list of what still runs under the lock is replaced by this record's: the supervisor was
  removed from it by ADRs 0117, 0119 and 0126. AGENTS.md "The Core lock" is updated once, in the change of step 5 (the
  first that moves something and adds a ratchet), to point here and to say what is left under the lock.
- After step 2, terminal **output and acknowledgements** no longer depend on the cost of any request.
  Keystroke echo still takes the Core lock (the input is recorded and its receipt is made durable under
  it: two commits per key), so it improves with step R and step 4, not with step 2. After step 1 a
  machine with open panes and nothing happening is idle. After steps 4 and 5 a stalled disk or a stalled
  mount delays the request that asked, and nothing else.
- Each change can be reverted alone.
- The relative order of a stream notification and a durable event for one pane is not a property of the
  lock (it never was; the outbox orders by priority). Clients do not depend on it; this ADR lists it so a
  client change does not start to.
