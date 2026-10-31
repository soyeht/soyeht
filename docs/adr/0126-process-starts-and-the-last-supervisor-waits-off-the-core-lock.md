# ADR 0126: The starts of processes and the last supervisor requests wait off the Core lock

- Status: accepted
- Date: 2026-10-30

## Context

ADR 0117 moved the terminal path's requests to the supervisor off the Core lock, and ADR 0119
the polls of the agents, of their terminals and of the language servers, and every close. It
left thirteen callers of `Core::pty_request` under the lock, and three restarts that waited,
under the lock, for the close of the process they replace (`Core::close_and_wait`). A supervisor
that was slow or stopped still froze every terminal and every request on every connection for
the client's timeout (five seconds) whenever one of those ran: the creation of a pane, the
recovery of a pane or an agent after a lost link, a restart after a probe or a role change, a
task's pane, an inline terminal an agent opened, a language server.

Four of the thirteen were the `Spawn` of a process (a pane, an agent, an inline terminal, a
language server). ADR 0119 left them because each caller returns the supervisor's error to a
recovery, a retry or a request, and because the pane they start had no state between the
reservation of its identity and its process. This record says what that state is, and moves the
four, the three restarts, and two lookups of the registry that did not need to be requests.

## Decision

No second mechanism: the start is a `Supervisor::call` in the work of an `Outcome::Offload` (a
request that answers a client) or of a `Core::spawn_background` job (the engine's own starts), and
its answer is settled under the lock with `Core::settle_supervisor` (ADR 0117).

**A pane that is starting is a pane with an identity and no process.** The identity (instance
key, generation, stream, sequence base) is reserved and saved before the request is sent, as it
always was; what is new is that the request is not answered under the lock. A pane in that state
is the one a failed start already left waiting for its retry (`spawn_retry`), so everything that
reads a pane already tolerates it: nothing polls a pane with no `instance_id`
(`drain_panes`), `pane.input` answers "Pane is recovering", the tick's retry and the reconciliation
of a new link start it. No new state was added to a pane, to the store or to the wire.

`process_start.rs` is the whole mechanism:

- `Core::launch` takes, under the lock, the link, a `Turn` in the order of the sends to the
  process being replaced (if any), and the request: a `Launch`. `Launch::send` waits for the
  turn and asks the supervisor, and runs off the lock. `Core::started` takes the answer back under
  the lock and says `Process`, `Failed` or `Stale` (the link it went through is not the engine's
  any more, so the answer says nothing about the one that replaced it).
- `Core::spawn(id)` and `Core::spawn_after(id, replaced)` start the process of a pane as a job
  keyed by the pane **and its identity** (`start:{pane}:{instance_key}`), so a start that is
  asked while one for the same identity runs asks for nothing more, and a pane given another
  identity meanwhile is started under it at once. When the job ends, `finish_pane_start` adopts the
  process into the pane (`Core::adopt`), or, when the pane closed or got another
  identity while the request ran, closes the process it got (`close_process`) and starts the pane
  again if it still wants a process. A failure is `settle_spawn`, as before: retried by the tick,
  or the pane is stopped when the supervisor refused.
- A start that ends `Stale` is scheduled as one more failed attempt of the pane
  (`schedule_spawn_retry`), because the job key of its identity is still taken while it applies;
  the reconciliation of the link that replaced it starts the panes it finds with no process, and
  the supervisor makes a process once per instance key, so a start asked twice gets the same
  process.
- `Core::spawn_then_reply` is `pane.create`: the request carries the `Launch` in an
  `Outcome::Offload`, and its reply is the one it always gave, sent once the supervisor has made
  the process or failed to. So a client that creates a pane and then types into it on the same
  connection still finds the process, and the wait holds neither the lock nor the requests of
  other connections. `pane.create` returns an `Outcome` now. So does `pane.auth.start`, which
  opens a sign-in terminal and answers once its process has started: the sign-in can be
  cancelled the moment the reply arrives, and the process the client was promised exists.

**The restarts wait on a turn, not on the lock.** The restart after a probe, after a role change
and for a task closed the process they replace and waited, under the lock, for the close to be
sent. They now call `Core::close_process` and start the replacement with `spawn_after`, which
takes its `Turn` after the close's, so the new process is requested only once the close has been
sent: the same order, kept by the order of the sends to that process, with the wait in the
background job and not under the lock. `close_and_wait` is gone.

**Inline terminals and language servers** start the same way (`start_process`). An agent's
`terminal/create` request stays open while the process starts and is answered by the job's apply
(`finish_inline_terminal_start`): the terminal gets its process, or the agent is told the harness
is unavailable when the supervisor says so. A failure that proves nothing (the link, a storage
error) leaves the request open and the next poll starts it again, as the reserved spawn key
always allowed. A language server is adopted when its process is made
(`finish_language_start`); a failure leaves it unavailable and fails the editor requests waiting
for it, as a failed poll does (`language_failed`, which both use now).

**Two lookups that were requests are not.** The exit status of a sign-in terminal was read from
the registry (`List`) when the terminal ended; the pane's drain had recorded it already
(the pane's `exitCode`), so `finish_login_terminal` reads that. A pane that closed before its process
was recorded asked the registry for the process of its instance key; a start that outruns its
pane's close now ends by closing its own process, and a process lost on the way is found and
closed by a job (`close_unrecorded_process`).

**The writes to a process are jobs that take a turn** (`Core::start_input`). One shape serves the
three: the request carries `Request::Input` and the instance, the job takes the sends queued for
that process (`take_later_for`) and a `Turn`, waits for the turn, sends the queued ones and then
the bytes (`Supervisor::send_input`, shared with `pane.input`), and the apply settles the
receipt (`Core::settle_input`, also shared) and hands `done` the bytes the supervisor accepted. A
receipt that fails on a link that was replaced meanwhile is dropped.

- **The outbox of an agent** (`flush_agent_outbox`) sends one page as a job keyed by the page
  (`outbox:{pane}:{operation}:{attempt}:{offset}`), so the apply of one page can start the next. When
  the page is accepted, `finish_outbox_page` checks that the oldest unsent message is still the one
  the page belongs to (the process may have been replaced), makes the advance durable, confirms the
  receipt and starts the next page. The poll starts the job on each tick that finds a message
  and nothing on its way, so a message goes out at the pace of the round trips. A failure is
  settled as the poll settled it (`settle_agent_poll`, which the tick uses too): a process the
  supervisor lost is recovered, a failure of the pane stops it, one that every pane shares is
  returned.
- **A language server's pending writes** (`deliver_language_write`) are one job each, keyed by
  the operation. They stay pending, and stored so, until the supervisor has accepted all the bytes;
  a write accepted by a server that was restarted meanwhile only confirms its receipt. A failure
  leaves the server unavailable (`language_failed`).
- **The requests a gap hides** (`recover_agent_requests_before`) are read by a job that lists
  the requests the supervisor holds and pages through the bytes of the ones the engine does not
  have (`fetch_pending`, with the bounds it always checked). The poll that meets the gap starts
  the job and leaves the gap for the next tick (`agent_output_gap` and
  `skip_interrupted_agent_record` say `false` until the answer is in `Core::reads`);
  the apply of the answer into the agent (`apply_recovered`) is the loop that was there.

**A harness switch is a step per job.** The state kept between ticks is the runtime that already
was: the candidate, its phase and its decoder. What changed is that the tick no longer talks to
the candidate. `advance_harness_switches` starts the next step of each switch as the job
`switch:{pane}`, unless one is on its way:

- With no candidate, the step builds everything under the lock (`draft_switch_candidate`: the
  state, the request to start it, the `initialize` message) and the job starts the process and
  writes `initialize`. `finish_candidate_spawn` records the candidate before it settles the write,
  so a failure after the process exists retires it through the ledger.
- With a candidate, the job writes the message the last step queued (`Candidate::queue`, which
  replaced writing from the handler of the answer) and reads the candidate's output
  (`write_candidate` and `read_candidate`, which wait and so run off the lock);
  `finish_candidate_read` decodes the output and takes the handshake's next step, exactly as the
  tick did. A write or a read that fails, or that ends on a link that was replaced, ends the
  switch and restores the original session: a write is never repeated, since some pages of it may
  have reached the candidate.

## Trade-offs

- **A started pane is adopted on the tick after its answer, not in the call that asked.** Except
  for `pane.create`, which still answers after the adoption, a caller that used to find the
  process the moment `spawn` returned (a task's pane, a recovery) finds it a tick (and a thread
  hop) later. Nothing read it in between.
- **A failure of the supervisor no longer fails the caller of `spawn`.** A task dispatch, a
  recovery after a probe or a role change, and the reconciliation of a link used to see the error
  of the start and fail their step. The error is settled where it comes (`settle_spawn`), which
  every one of them already did for the failures that could not be returned: the pane is retried
  by the tick, and stopped when the supervisor refused. A non-refusal during a reconciliation no
  longer fails the reconciliation (and with it the link): the pane alone is retried.
- **The reconciliation of the language servers no longer fails on a start that errors.** A
  non-refusal error of a server's restart used to fail the reconciliation (and drop the link to
  try again); it is now the server's own failure, settled when the supervisor answers
  (`language_failed`: unavailable, with the waiting editor requests failed). So a refused start
  is logged as `engine.lsp_failed` and no longer as `engine.lsp_not_restarted`, which only a
  restart that could not begin can still reach.
- **A repeated `idempotencyKey` during a create that is still starting returns the stored reply
  at once**, before the process exists, as the first request's reply would have been if it had
  not waited. The pane is in the state a pane waiting for its retry has, which clients tolerate.
- **A write that lost its link is sent again by the tick.** A language server's write whose job
  ended on a replaced link stays pending, and the tick sends the pending writes that have no job
  (the operation key is the same, so the supervisor answers once). A result that arrives for a
  write the server no longer holds, or for a process the pane no longer has, is dropped (the
  receipt is confirmed): it says nothing about the new process.
- **An agent's message, a gap's requests and a switch's steps wait for the tick that applies
  them.** Each is a round trip and a thread hop later than when it was a call under the lock
  (the first page of a message goes out on the tick after the poll that found it, the next pages
  as their receipts come back). A switch has 30 seconds to complete either way.
- **A write of a switch that fails is not repeated**, and a request recovery that finds that the
  engine recorded a request meanwhile reads it again.
- **The jobs share the four workers of the background pool**, as the terminal's reads and the
  closes do. A start waits on a worker while it waits for the supervisor, and for the turn of the
  close it follows; the close is queued first, so it is never behind the start that waits for it.

## Consequences

- Nothing waits for the supervisor with the Core lock held. `Core::pty_request` (now test-only)
  and `close_and_wait` are gone, and `tests/blocking_io_rule.rs` holds a table of the files that
  wait for the supervisor (`call`, `call_each`, `send_input`), each the work of a job or an
  offload: a wait anywhere else, or one more in a file, fails it. `tests/core_lock.rs` creates a
  pane while the supervisor is stopped and measures a status request on another connection.
- The unit tests of `process_start.rs` run the jobs on the pool with a supervisor that takes a
  second to answer, and prove that the call returns at once, that a second start asks for
  nothing, that a pane that closed or changed identity does not take the process it got, that a
  start answered on a replaced link is left to the retry, that the replacement of a process asks
  after the close, and that `pane.create` answers only when the process has started.
- The tests of the later steps are as direct: an input job returns at once and sends the queued
  size first (`supervisor_handle`), a gap's requests are read off the lock (`recovery`), a switch
  step returns at once while the supervisor takes its time to start the candidate
  (`switching`), and `tests/core_lock.rs` delivers a prompt while the supervisor is stopped.
