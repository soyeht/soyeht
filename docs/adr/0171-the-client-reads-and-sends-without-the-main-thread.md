# ADR 0171: The client reads, matches and sends without the main thread

- Status: accepted. Changes where the client's connection work runs; nothing changes for the engine or the wire.
- Date: 2026-11-20

## Context

A Release build of the Mac app with agents streaming in workspaces that are not shown kept the main thread
busy for seconds at a time (the window's views read what every pane writes). The engine answered every request
in 0 to 5 ms, and still two things went wrong: keys typed into a visible terminal reached the engine 1 to 33
seconds late (all of them at once, in the worst run, when the stream ended), and the app dropped its
connection (53 request timeouts and 8 connections in 3 minutes, and a modal "Connection interrupted").

The cause was in the transport, not in the views. The wire, the table of requests waiting for an answer, the
send queue and the reader all lived on the main actor. Each frame the reader took cost it several main-actor
turns (two reads, a parse and the apply), each behind everything else the busy main thread had queued. A
request's deadline was a main-actor task too, so when the main thread came free the expired deadline and the
unread answer ran in no useful order, and the deadline closed a healthy connection. A key was sent as a
request whose answer had to come back through the same turns before the next key could leave.

## Decision

- **The wire runs on no actor.** `Wire` takes bytes off the socket in blocks, frames them from a buffer and
  resumes its reader and writer directly from the connection's callbacks. `SendQueue` is its own actor.
- **An answer is matched where it is read.** `EngineReader` runs on no actor: it frames, parses a JSON frame
  and, for a reply, takes the request out of `PendingRequests` (a lock around a table) at once, which also
  cancels its deadline. A deadline therefore never fires for an answer the app already holds, however long
  the main actor takes to apply it. A request that the engine truly leaves unanswered still fails after
  `ClientPolicy.requestTimeoutMs`, and a connection that answered nothing for that long is closed.
- **A request behind others is not a silent engine.** The engine serves a connection's requests one at a
  time (each takes as long as its wait for the Core lock, tens of milliseconds with many agents streaming),
  so a burst of the client's own requests queues in the engine. When a deadline passes and the engine has
  answered other requests since this one was sent, the request waits another window, up to
  `ClientPolicy.capsRequestWindows` windows in all, and then fails alone with the connection left open.
  The Mac app's per-pane queue refreshes (`pane.queue.list`, about 15 a second with 21 hidden agents
  streaming) were the burst in the run that measured this.
- **The main actor applies, in order, in batches.** Everything the reader does not finish itself (events,
  terminal and attachment frames, and the replies whose caller reads state events changed) goes to
  `IncomingQueue` in the order it was read. `EngineClient` takes the whole backlog at once and applies it in
  order, giving the window a turn after each `ClientPolicy.applyBudgetMs`, so a backlog does not stop the
  window from drawing and the window's own work coalesces what a batch wrote. The replica rules (event
  cursor, stream gaps, snapshot parts, reconnect hydration) are unchanged: they run on the main actor over the
  same sequence.
- **The backlog is bounded.** More than `ClientPolicy.capsIncomingQueueBytes` waiting means the main actor
  has lost the screen: the connection ends with `invalidStream`, every request whose answer was read but not
  applied fails, and the reconnect hydrates the replicas again. The engine never sees this client as slow
  while the queue has room, so `slow_consumer` does not close the connection of a busy window.
- **Terminal input has its own path.** `EngineClient.input` checks the pane on the main actor and queues the
  bytes in `TerminalInputQueue` with no suspension. A task of no actor sends them in pieces of
  `ClientPolicy.terminalInputChunkBytes`, in order, each after the engine said how much of the previous piece
  it took, and its answers are `immediate`: they resume the sender from the reader, not through the main
  actor. Connection loss is retried with the same idempotency key, as `mutation` does.
- **The client does not queue a burst in front of its own keys.** At most `ClientPolicy.capsRequestsInFlight`
  requests wait for the engine at once (`RequestSlots`); the others wait in the client, in order, and their
  deadline starts when they are sent. Terminal input skips the line, so a key waits at most for the requests
  already sent, not for a burst of queue refreshes (in the run that measured this, keys reached the engine
  24 to 34 seconds late behind about 15 refreshes a second). Slots are given back when the answer is read,
  not when the main actor applies it.
- **Which answers wait for the main actor.** `ReplyDelivery.ordered` (the default) resumes the caller from the
  main actor after the events read before the answer, because callers read the state those events changed.
  `immediate` is for a request whose answer reads nothing the main actor holds.

## Consequences

- With the main thread busy, a key reaches the engine in the time of a local round trip, and a request is
  answered or fails on the engine's own account.
- The apply order is the read order, as before; only when it happens changed. A caller that awaits an
  `ordered` request still resumes after the events that preceded the answer.
- A main actor that stays behind for longer than the queue holds costs a reconnect, not a longer queue. The
  limit is in the contract (`x-clientPolicy`), measured in the bytes of the frames.
- The Windows and Linux client reads on a thread of its own and matches answers there, but its reader waits
  when the 1024 events it hands the interface are unread, so a stalled interface thread also stops the
  answers. It is not changed here.
