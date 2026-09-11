# 0024 — Interruption pauses the durable agent queue

Status: accepted for slice 2.

Contract 04's September 11 addendum requires interruption to pause the queue.
Persist that flag in the agent checkpoint. A cancellation cannot be followed by
an automatic dispatch from the waiting queue, even after an engine restart.
The active turn still ends only on its native response or its failure deadline.

Expose `queuePaused` in the agent snapshot and `paused` in `pane.queue.list`.
`pane.cancel` publishes `pane.queue.paused` when the flag first changes, with
reason `interrupted`. `pane.queue.resume` is a separate idempotent mutation and
publishes `pane.queue.resumed` only when the flag actually changes. Both events
carry `paneId` and `paused` in the normal durable event envelope.

Sending another prompt does not implicitly resume a paused queue. The client
shows the pause explanation and an explicit Resume action. Resumption still
honors configuration application, active requests and human draft presence.
Existing stored checkpoints without the flag default to an unpaused queue.

The new names are additions to protocol version 1 before any client release.
Shared Rust, Swift and TypeScript vectors include the resume request, result
and event. Runtime validation cancels a turn with another prompt waiting,
restarts the engine, proves the queued prompt stayed untouched, and then resumes
it once despite a repeated request.
