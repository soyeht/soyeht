# ADR 0014 · Full queue and operation logging

Status: accepted. Scope: slice 1.

When exceeding 8 MiB, the connection discards its queue and attempts to write one
JSON-RPC error response with `id: null`, a numeric code and `kind: slow_consumer`.
It then closes the transport. The error does not wait for space in the already
full queue. Snapshot writers are released when pending parts are discarded.

If the transport has stopped accepting bytes, delivery of the reason to the
client cannot be guaranteed. The final error attempt has a five-second deadline.
Ordinary writes remain governed by the 8 MiB queue budget instead of a separate
time deadline. Closing remains mandatory. The client treats both the error and
the disconnect as reasons to fetch a snapshot and subscribe to events again. No
continuity is promised for bytes left in the discarded queue.

Each request receives an operation identifier created by the engine. Before
dispatch, the engine writes `engine.operation_started` with that identifier,
connection, schema-known method, validated pane route and UTC start time. A
completed request writes `engine.operation` with the same identity plus its
result, error kind, finish time and duration. An unmatched start therefore remains
visible if dispatch never returns. A catalogue request handed to the shared
catalogue worker keeps its operation identity: its `engine.operation` record is
written when the worker answers it, including a request that shares another
request's scan, or at once when the catalogue queue rejects it. Neither record contains parameters,
client-supplied IDs, free-form error text, keystrokes, challenges, codes, tokens
or keys. A method outside the schema appears as `unknown`, without copying the
received text.

An unmatched start is not itself a failed request: ordinary work may still be in
progress. For the ordinary `EngineClient` path, this diagnostic calls an interval
overdue only after the existing twelve-second client deadline has elapsed. The
classification is declared before the next Harnesses reproduction. Pairing uses
its separate timeout and is not classified by that threshold. Pairing start and
finish records is also confined to the current engine life, beginning at the last
`engine.ready` line; records from earlier lives in the append-only log never match
the current life. If the engine life ends first, an unmatched start is recorded as
interrupted by shutdown rather than a client timeout.

The UTC `startedAt` and `finishedAt` fields come from the wall clock so operations
can be correlated across logs. `durationMs` comes independently from a monotonic
clock so wall-clock adjustments cannot corrupt elapsed time. Consequently,
subtracting the timestamps need not equal `durationMs`, especially across a clock
adjustment.

The queue control checks the memory limit, numeric error before end of stream,
release of bytes and retention of the first close reason. The logging control
checks that arbitrary text in a method name does not enter the log. Every accepted
connection emits one `engine.connection_closed` record with a bounded reason.
