# ADR 0032: Typed block retry timing

Status: accepted for slice 3 on 2026-09-12.

## Context

The task contract requires two delayed retries for an `external_error`, then
conversion to `missing_information`. It does not define the delay or whether a
scheduled retry counts before the prompt is durably accepted.

## Decision

An external error schedules its next retry for 30 seconds later. A retry counts
only when its automatic prompt and the incremented `retryCount` commit in the
same transaction. A full prompt queue postpones the deadline without consuming
a retry.

The successful enqueue clears the deadline. If the executor reports the same
external error again, the engine keeps the counter and schedules the next
deadline. After the second retry reports the same error, the next deadline
converts the block to `missing_information` and creates an attention item.
Changing the error reason starts a new two-retry sequence.

## Consequences

Restarting the engine cannot duplicate a retry or lose an accepted retry
prompt. Long-running retry turns are not escalated while they are still in
progress. The fixed 30-second delay is part of slice 3 behavior and may become
a bounded profile setting in a later contract change.
