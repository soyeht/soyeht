# ADR 0080: An idle engine does work on events

- Status: accepted
- Date: 2026-10-07

## Context

A test machine with no app connected and every agent idle kept the engine at
about 50% of a core. A profile of the tick (every 10 ms, under the Core lock)
showed it doing the same work over and over:

- Every agent pane ran the whole advance pass on every tick: it cloned the
  agent state (with its config option vectors) several times, copied the
  stored pane, and ran four SQL queries that always returned nothing (open
  requests twice, the outbox, and again for terminals and files).
- Seven task and permission polls ran one SQL query each per tick (task
  reviews, dispatches, feedback, blocked tasks and retries, due loops, expired
  web permissions), and every query was prepared again each time.
- A faulted agent sent a close request to the supervisor on every tick, and a
  healthy one sent an acknowledgement of the same position on every tick.
- Cleanup of retired agent processes copied the state of every agent on every
  tick.

None of it changed anything. Slowing the tick would also slow the terminal
drain, which is the latency path, so the tick rate is not the fix.

## Decision

The tick drains output and nothing else when nothing happened.

1. **An idle agent settles.** An agent that is ready, with no turn, queue,
   operation, terminal, pending call or open request, and an empty outbox,
   records that it needs nothing until its state is committed or an open
   request expires (`agents::idle`). Every commit of the agent's state ends
   the settled period (`Idle::touch`). The tick still drains the agent's output
   but skips deadlines, the advance pass and the outbox. Agents that have work
   keep the full pass.
2. **A poll that found nothing rests.** A poll that is a pure function of the
   database rows and the time records the database change counter and, when
   rows can fall due, the next due time (`soyeht-store` `resting`). It asks
   again only after a write or when that time arrives. Polls that also read
   in-memory state (task feedback decisions) do not rest on that state.
3. **Repeated requests are not repeated.** An acknowledgement is sent once per
   process position, and a close once per process. Cleanup of retired
   processes visits only the agents that retired one.
4. **Prepared statements are cached** for the queries that remain on the tick.
5. **Closing ends the whole tree.** A guarded (agent or language server)
   instance is closed with its process group and every descendant that left
   the group, such as a shell tool that started its own session. Terminals
   keep the group-only rule, so a daemon the user detached stays.
6. **Nothing is left unowned.** When the engine reconciles with the
   supervisor, it closes every open agent process that no pane, retirement
   ledger or pending harness switch refers to.

## Consequences

- An idle tick runs no SQL, copies no agent state and makes one drain request
  per pane. `agents::idle` tests assert this with a statement counter in
  `soyeht-store` (`test-support`) and a clone probe on the agent state; the
  `idle_cpu` bench measures the process.
- New per-tick work must be event driven or rest in the same way. A poll that
  reads state outside the database must say what wakes it.
- A resting poll sees the engine's own writes at once. A write by another
  process (the command line tool) wakes it within five seconds, the longest a
  poll rests.
