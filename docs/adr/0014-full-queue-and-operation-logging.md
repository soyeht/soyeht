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

Each request receives an operation identifier created by the engine. The JSON
log contains that identifier, connection, schema-known method, result, error kind
and duration. It contains no parameters, client-supplied IDs, free-form error text,
keystrokes, challenges, codes, tokens or keys. A method outside the schema appears
as `unknown`, without copying the received text.

The UTC `startedAt` and `finishedAt` fields come from the wall clock so operations
can be correlated across logs. `durationMs` comes independently from a monotonic
clock so wall-clock adjustments cannot corrupt elapsed time. Consequently,
subtracting the timestamps need not equal `durationMs`, especially across a clock
adjustment.

The queue control checks the memory limit, numeric error before end of stream,
release of bytes and retention of the first close reason. The logging control
checks that arbitrary text in a method name does not enter the log. Every accepted
connection emits one `engine.connection_closed` record with a bounded reason.
