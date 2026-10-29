# ADR 0117: Supervisor requests wait off the Core lock

- Status: accepted
- Date: 2026-10-28

## Context

ADR 0079 says no code holds the Core lock across blocking IO. The request to the
supervisor was left out of it. `Core::pty_request` called the supervisor client with
`&mut Core` in hand, so with the Core lock held. The client writes a request and reads
the answer on a Unix socket (a loopback endpoint on Windows) with a timeout of five
seconds each (`soyeht_ptyd::client::IO_TIMEOUT`). The tick, which holds the lock and
runs every 10 ms, asked the supervisor for the output of every terminal pane and for
every acknowledgement, and the input of a terminal went through the same call. A
supervisor that was slow, stopped or wedged therefore froze every terminal and every
request on every connection for up to the timeout, and for as long as the supervisor
stayed in that state, since the tick asked again at once.

`tests/blocking_io_rule.rs` and `tests/core_lock.rs` did not catch it: the first looks
for external commands, the second for a slow usage source. A supervisor stopped with a
signal froze a status request on another connection for 4.6 s before this change.

There were 55 production calls of `pty_request`, in the terminal path (`terminal_drain`,
`input`, the size sent on a resize) and in the engine's other jobs: the agent poll and the
agent's terminals (`agents/`), the language servers (`languages/runtime.rs`), the harness
switch, the reconciliation after a reconnect, spawn and close.

## Decision

**The link is a handle.** `Core::ptyd` is a `Supervisor` (`supervisor_handle.rs`): a cheap
clone that owns the connection behind a mutex of its own (an `offload::SharedPort`, the
shape the git and `wt` ports already have). `Supervisor::call` sends one request and waits
for the answer. It is called off the Core lock, in the work of an `Outcome::Offload` or of a
`Core::spawn_background` job, after the caller copied its inputs under the lock; the
answer is taken back under the lock by `Core::settle_supervisor`, which drops the link when
the error says it is gone (`errors::supervisor_link_lost`, the one rule) and records
`ptyd.lost`. It drops the link only when the link in the Core is still the one the call
used: a call that outlived its link says nothing about the one that replaced it.

The mutex of the connection orders the users of the one stream, one request and answer at a
time. It is never taken together with the Core lock by a path that moved.

**What moved** (the terminal path):

- The read of a terminal pane's output. `drain_terminal` no longer asks under the lock: it
  starts one background job per pane, which asks the supervisor (and the exit
  status when the output says the process exited) with no lock. The job is applied under the
  lock, which puts the output into the screen as before. A read that no longer fits the pane
  when it is applied (its process was replaced, the pane closed, its screen moved, its
  capture is waiting to be restored) is dropped; the supervisor still holds the output and
  the next read starts from the screen's offset.
- The acknowledgement. The pane's flush job sends it after the flush, with no
  lock; without recording it sends it at once. A refusal is owed again, as before.
- `pane.input`. The durable intent is still written under the lock before the irreversible
  write. The write goes to the supervisor in the work of an `Outcome::Offload`; the receipt
  is made durable under the lock when it is applied. A connection answers its next request
  after the previous reply, so inputs keep their order, and an input comes before the resize
  that follows it.
- The best-effort requests nobody waits for: the confirmation of an input receipt and the
  size sent after a resize. `Core::send_later` queues them and the background job
  `supervisor.later` sends them in order. A size not yet sent is replaced by the newer one
  for the same process. The same queue carries the other requests whose failure the caller
  already ignored and that nothing after them depends on: the close of a pane's processes, the
  flow-control acknowledgement of a harness switch, and the close and confirmations of a language
  server marked unavailable. A failure is logged under `engine.supervisor_follow_up_failed` by
  `settle_supervisor_call`, once per change of kind.

**Latency of the terminal.** A job is applied by the tick that follows its end, so a read
that finds output would show it one tick (10 ms) later than a read made inside the tick.
A job whose result has news (output, an exit, an error) therefore wakes the tick
(`Core::spawn_prompt_background`), which applies it and pumps the streams at once. A read
that finds nothing does not wake it, so a quiet pane makes the tick no busier than before.
The output of a pane is shown after the read that finds it, as it was.

**The deadline of an offloaded request** is six times the client timeout
(`supervisor_handle::DEADLINE`, 30 s): the wait behind the requests already using the
connection plus its own write and read. A request that outlives it is answered with an
internal error; for an input the durable intent stays, and the retry with the same key
sends the same operation key, which the supervisor deduplicates.

**What did not move** is counted. `tests/blocking_io_rule.rs`
(`requests_to_the_supervisor_under_the_core_lock_do_not_grow`) lists the files that still
call `pty_request` and how many times each does, and fails on a new call, on a file that
is not in the list, and on a count that went down without the list following it. It also
fails on a `call_supervisor` outside the port and the handle. `tests/core_lock.rs` stops
the real supervisor with a signal and measures a request on another connection;
`supervisor_handle.rs` holds the same with a stub that delays its answer.

The remaining 44 calls, by group:

| Group | Files | Calls |
| --- | --- | --- |
| Agent panes: the poll, the drain and the terminals of an agent | `agents/terminals.rs`, `agents/driver.rs`, `agents/delivery.rs`, `agents/recovery.rs`, `agents/failures.rs`, `agents/lifecycle.rs`, `agents/orphans.rs`, `agents/authentication.rs` | 19 |
| Harness switch | `agents/switching.rs`, `agents/switching/candidate.rs`, `agents/switching/commit.rs` | 6 |
| Language servers | `languages/runtime.rs` | 8 |
| Start, reconnect and reconcile: spawn, adopt, close, list, pending inputs | `core.rs`, `spawn_retry.rs` | 8 |
| Close of a pane and of a role's process, dispatch step | `panes.rs`, `tasks/roles.rs`, `tasks/dispatching.rs` | 3 |

The agent poll runs on every tick for every agent pane and is the next to move. The spawn
after a pane is created is the one user action that still waits under the lock; it follows
with the start-up group, which has to settle its state in order.

## Trade-offs

- **A handle per link, not one thread with a request channel.** A dedicated supervisor thread
  would give strict first-in-first-out order across callers. The mutex of the connection gives
  each caller the stream in turn, and the callers that need an order among themselves already
  have one: a connection answers one request at a time, a pane has one read and one flush job
  at a time, and the follow-ups go through one queue. A thread would add a channel, a
  shutdown and an error path for a stream that already serializes its users. The mutex also
  reuses `SharedPort`, which the other ports use.
- **A slow supervisor still holds up what needs it.** The work off the lock waits for the
  connection, and so does an agent poll that has not moved, which then holds the Core lock
  while it waits for the connection's mutex. The terminal path and every request that does
  not use the supervisor are free; the agent panes stay as slow as the supervisor until their
  group moves. This is the same exposure as before for those callers, not a new one.
- **One more tick between a read and its application in the worst case**, bounded by waking
  the tick for a result that has news. A pane that was removed or replaced between the start
  of a read and its application drops the read.
- **Errors of a read are isolated per pane.** A read that fails is retried after the cooldown
  of its job (one second) and logged once per change of kind as a subsystem failure; it used
  to stop the drain of the panes after it in the same tick. A lost link is dropped at once
  when the error is applied.
- **Follow-ups that fail are logged, not retried.** The size of a terminal and the
  confirmation of an input were `let _ =` before; a failure is now visible in the log.
  Nothing depends on them: the size is durable, and an unconfirmed receipt is answered by the
  supervisor's registry at the next reconnect.

## Consequences

- The tick holds the Core lock without waiting for the supervisor on the terminal path.
- A new request to the supervisor is written as an offloaded call. The count in
  `blocking_io_rule.rs` falls to zero as the groups above move, and the `pty_request` method
  is removed with the last of them.
- `soyeht_ptyd::client::IO_TIMEOUT` is the one place of the five seconds.
