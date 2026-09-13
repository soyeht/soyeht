# ADR 0052: Editor change request delivery

- Status: accepted
- Date: 2026-09-13

## Context

An editor line comment must reach the agent that authored the active file
review. When that pane owns a task, the same fact must also appear on the task
card as a review change request. Delivery, task state, retry behavior, and a
closed pane need one consistent rule.

## Decision

`editor.review.comment` addresses the pane stored in the active durable file
review. The caller cannot choose another pane. The engine validates the range
against the current clean review revision and rejects an invalid UTF-16
position before any write.

For a live author pane, the engine puts the request in the existing durable
feedback queue with feedback priority. If that pane owns an active task, the
same transaction writes a task comment, sets the task badge to
`system_turn`, sets its phrase to `review requested changes`, increments its
version, records the idempotency receipt, and publishes all related events.
The operation does not move the task between columns.

If the author pane is closed or dead, the engine records an idempotent result
with `delivered: false` and changes neither a queue nor a task. A retry with
the same key returns the same result and comment identifier.

The request range refers to the active review at execution time. The protocol
does not carry a separate base revision for this operation, so clients must
show the comment action only for the review currently held in their replica.

## Consequences

Editor feedback and CI feedback share one ordering and persistence mechanism.
A human draft or permission request continues to delay automatic injection
without losing the queued comment. The task and pane cannot disagree after a
successful transaction, and a dead target is reported without pretending the
message arrived.
