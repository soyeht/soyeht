# ADR 0119: The agent polls and every close wait off the Core lock

- Status: accepted
- Date: 2026-10-29

## Context

ADR 0117 moved the terminal path's requests to the supervisor off the Core lock and left 43
callers of `Core::pty_request` under it, counted by `tests/blocking_io_rule.rs`. It named the
agent poll as the next to move. The agent poll is the hot one: on every tick, for every agent
pane, it asked the supervisor for the agent's output with the lock held, as did the poll of
every language server and of every inline terminal an agent had open. A supervisor that was
stopped froze every request on every connection for the five-second timeout, as the terminal
path did before ADR 0117, as soon as one agent existed.

## Decision

No second mechanism: everything below is a `Supervisor::call` in the work of a
`Core::spawn_background` job, settled under the lock with `Core::settle_supervisor`, and the
sends nobody waits for go through `Core::send_later` and its turns (ADR 0117).

**Reads for a poll that keeps its place in the tick (`read_ahead.rs`).** The poll of an agent
(`poll_agent`), of an agent's inline terminals (`poll_agent_terminals`) and of a language server
(`poll_language_server`) is a state machine that has to run in the tick, in order with the steps
around it, and that holds the agent's state while it does. It cannot be a job that ends
whenever it ends, as the shell pane's drain is (`terminal_drain`), which puts its output straight
into a screen. So the read is the job and the poll stays where it is:

- A poll asks `Core::take_read(owner, instance, offset)`. If a read for exactly that process and
  offset has ended, the tick applied it into a table (`Reads`) and the poll takes it, with its
  error if it failed. If not, a read is started and the poll does nothing this tick.
- A poll that took a read starts the next one at once, at the offset the output it has just put
  into the screen ends at, and goes on with the rest of its work while the read runs. So an agent
  that is producing output has a read on its way at every tick.
- A read that finds output, a gap or an error wakes the tick, as the shell pane's read does: the
  tick applies it, polls the agents at once and sends what that brought (`Core::poll_woken`), so
  output is not left waiting for the next tick. The woken poll takes the reads that ended and
  starts none, and leaves the shell panes to the tick: the next read starts on the regular tick,
  so a process that never stops writing is polled at the pace of the tick and not at the pace of
  the round trip. A quiet read, an exit with nothing left to read, and every read of a language
  server do not wake it (a language server's output is not polled by the woken tick).
- A read asked for another process or offset (the process was replaced, the screen moved) is
  discarded when the poll asks. The supervisor keeps the output until it is told to drop it, so
  a read that is never taken costs nothing. The reads of a pane that closes are forgotten.
- The read is `terminal_drain::read_output`, the one the shell pane uses, including the lookup
  of the exit status that the inline terminals need (the poll no longer asks for the registry).
  The job is keyed per owner, so there is one read at a time for each.

**An idle agent is read at a slower pace.** An agent that has nothing to advance (ADR 0080:
ready, no turn, no queued work, no open request) is read once per `IDLE_PACE_MS` (100 ms), not
once per tick, and only after a read that found nothing. Anything that changes its state (a
prompt, an answer, an expiry) ends the wait at once, because the wait applies only while the
agent is settled. What it says meanwhile waits in the supervisor, which keeps it until it is
acknowledged. The ignored `idle_cpu` bench (an engine with two idle agents, release profile, on
a busy Mac with a load average of 10 to 20, `cargo test --release -p soyeht-engine --test
idle_cpu -- --ignored`, each run twice) read 5.0% and 4.9% of a core on the commit before this
change (`d1bda910` plus the fixtures of #8), 4.9% and 4.4% with the reads off the lock and no
pace, and 2.6% and 2.5% with the pace: below the 5% bound it states. The hand-off of a read
between threads costs the engine no more than the request it replaced; the saving is the
requests an idle agent no longer makes. The language servers are read on every tick still:
a server answers an editor request, and a person waits for that.

The acknowledgements of these polls (`Ack`) and the confirmations of delivered inputs
(`ConfirmInput`) are `send_later`: nobody waits for them, and the supervisor keeps the output a
little longer if one fails.

**Every close is off the lock, except three.** `Core::close_process` is the one way to close a process whose
failure the caller cannot return: a job that takes a turn for the process, sends the close, and
settles the answer under the lock with the retries that already existed (`settle_close`: logged,
tried again by the tick a bounded number of times, left to the next link when the link is gone).
It replaces the closes with `?` of a pane's recovery, of the restart after a probe and after a
role change, of a task's restart, of the children of a lost agent, of an agent's faulted
processes, of the orphans found at a reconnect and of a language server that is parked. A close
that failed used to fail the recovery it was part of; it is now retried by itself, so a process
that cannot be closed no longer keeps its agent from coming up. The process of a retired agent
is held by the retirement ledger until the supervisor has closed it, as before:
`cleanup_retired_agent_processes` closes them in a job per agent and takes the ones that closed
out of the ledger when it is applied (a close the supervisor keeps refusing is logged once per
change of kind, `engine.retired_close_failed`, and then not sent again before a delay that
doubles from 250 ms to 4 s), and the process of a candidate that failed to
start goes straight into the ledger instead of being closed first.

A process that the engine stops tracking is in the ledger first, because a close that fails, or
that finds no link, must not leave it running: the inline terminals of a lost agent, the ones of
a faulted agent (handed over once, and marked closed only then), and a terminal the agent
released. A kill of an inline terminal keeps the terminal in the agent's state, so a kill that
fails leaves a process the agent can still release; with no link it is answered with a failure,
not a success.

The sends of a process that is closing are not left to find it gone: `close_process` drops the
sends still queued for it, an acknowledgement replaces the one not yet sent as a size does, and a
close is not started when one is already on its way.

Three callers close a process that a spawn is about to replace (the restart after a probe, after a
role change and for a task). The replacement must not start while the process it replaces still
holds its session or folder. They first waited, under the lock, until the close had been sent
(`Core::close_and_wait`); ADR 0126 moved the wait into the replacement's own job, which takes its
turn after the close's, so no wait is left under the lock.

**The registry is read before the link is the engine's.** The connect job (ADR 0117) now also
reads the supervisor's pending inputs and processes through the new handle, and hands both to the
reconciliation, which runs under the lock with nothing left to wait for. No request goes to a new
link before the link is the engine's, so the registry is as it was when the reconciliation runs,
except for a process that ended meanwhile, which the next read of it says. `Core::open`, which
connects before the Core is shared, reads it in place.

**Sizes and confirmations at adoption** (`Resize` after a spawn, `ConfirmInput` of the receipts
nobody needs) are `send_later`: the size is durable and the receipt is reconciled at the next
link.

## What did not move, and why

Thirteen callers of `pty_request` remained, counted in `tests/blocking_io_rule.rs`. Each is a step
whose next step needs its answer and whose state has to be settled in order, which is a decision
and not a move. ADR 0126 moved the four spawns and the two lookups of the registry (the rows that
begin with "Moved"); the others are as they were:

| Caller | Calls | Why it waits |
| --- | --- | --- |
| `Spawn` of a pane (`core.rs`), of an agent (`agents/lifecycle.rs`), of an inline terminal (`agents/terminals.rs`), of a language server (`languages/runtime.rs`) | 4 | Moved (ADR 0126). The process must exist before the pane, the agent, the terminal or the server is adopted, and the caller returns the error to a recovery, a retry (`spawn_retry`) or an agent request. Moving it means a pane that is "starting" between the reservation of its identity and its process, with the retry, the recovery and each request that reads the pane's process written for that state. It follows in its own change. |
| `Input` to an agent (`agents/delivery.rs`), to a language server (`languages/runtime.rs`), to a candidate harness (`agents/switching/candidate.rs`) | 3 | The bytes the supervisor accepted decide the durable advance of the outbox, of the pending write or of the switch, and the caller loops over the pages of a message. `pane.input` has the shape (the intent before, the receipt after, off the lock); the outbox needs it per page and in the order the tick delivers them. |
| `Spawn` and `Drain` of a candidate harness (`agents/switching/candidate.rs`) | 2 | A harness switch is a handshake of several steps with the candidate (spawn, read its answers, write, read again) inside a tick, each step deciding the next. It needs a state of its own, kept between ticks, before it can wait off the lock. |
| `PendingRequests` and `PendingRequestChunk` (`agents/recovery.rs`) | 2 | Recovery of the requests an agent had pending when its output has a gap, a loop of pages inside the decoding of the output. It runs after a gap, not on a tick. |
| `List` for the exit status of a sign-in terminal (`agents/authentication.rs`) | 1 | Moved (ADR 0126). One request inside the step that finishes a sign-in, after the person closed the terminal. |
| `List` to find the process of a closing pane that never recorded one (`panes.rs`) | 1 | Moved (ADR 0126). One request in the handler of `pane.close`, for a pane that closed before its process was recorded. |

## Trade-offs

- **An agent advances only on a tick that has its read.** A poll with no read ready returns
  before the advance. Reads are always on their way, and one that finds output wakes the tick, so
  output is polled as soon as the read that found it ends, which is no later than the next tick
  found it before. A reply to a write the poll made in a tick is read by the read that starts in
  a later tick, as before. The first step of an idle agent after its state changed starts a read
  and runs on the next tick: up to one tick (10 ms) and a thread hop later than before, once.
  With a supervisor that is slow or stopped, an agent waits for it, as it must (its next step is a
  write to the supervisor), and the rest of the engine does not. I did not measure the latency of
  a reply separately.
- **An idle agent hears the process after up to 100 ms**, not 10: what an idle agent says unasked
  (a notification) reaches the clients that much later. Its answer to a prompt is not delayed,
  since a prompt unsettles the agent.
- **The jobs share the four workers of the background pool**, as the terminal's reads do. A
  stopped supervisor holds them for one timeout, after which the handle is broken and every job
  waiting on it fails at once (ADR 0117).
- **A close that cannot be returned is retried, not returned.** A recovery used to fail, and be
  tried again on its own pace, when a process of the agent could not be closed. It now goes on, the
  close is retried (five attempts, then left to the next link), and a failure is logged under
  `engine.pane_close_failed`. A kill of an inline terminal that fails is logged under
  `engine.supervisor_follow_up_failed`, not returned to the agent that asked; the terminal stays,
  and so does the process in the ledger once released.

## Consequences

- Nothing a tick does on behalf of an agent, a language server or a close waits for the
  supervisor with the lock held, except the callers in the table above. `tests/core_lock.rs` stops
  the real supervisor with an agent running and measures a status request on another connection.
- The callers left are the table above, less what ADR 0126 moved. `pty_request` is removed with
  the last.
