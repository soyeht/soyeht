# ADR 0020 · Agent persistence transactions

Status: accepted. Scope: slice 2 storage used by the engine.

## Atomic ownership

SQLite schema 4 adds agent runtime checkpoints, a native outbox, native
requests, normalized transcript records and probe results. Migration uses the
existing backup and immediate transaction. Existing pane, process and database
identities are unchanged. Only the engine writes this database.

Queued prompt bodies live in separate ordered rows. A checkpoint contains
the queue revision and active prompt, but not a second copy of every waiting
body. The engine hydrates its in-memory queue from those rows on adoption.
Queue insertion/removal, revision, receipt and event share one transaction.
A full queue is not rewritten each time an output chunk arrives. Byte limits
apply to the original UTF-8 prompt; JSON escaping cannot reduce that capacity.

All storage writes take the engine's transaction. Receiving a complete native
record commits its byte checkpoint, runtime state and public events together.
Answering a request commits the winning decision, native reply and public
event together. The engine sends native bytes only after that commit.

An outgoing operation retains exact bytes, operation key, pane generation,
supervisor and process identity. The engine retries it only against that same
process, using ptyd's input deduplication. Each at-most-64-KiB write has an
attempt ID and byte position. The engine commits the accepted byte count
before confirming that attempt's ptyd receipt. Zero-byte acceptance advances
the attempt but not the position. After full delivery it deletes the outgoing
row durably, then confirms the last ptyd receipt. A replacement process
must never receive an old outbox operation. Reconciliation of a lost process
belongs to the engine; storage does not decide to repeat its effects.

An incoming request is identified by pane, generation and the first byte's
sequence. Its native JSON ID remains separate, including null versus an
integer or string. Replay retains the original engine request ID, deadline
and answer. Changed native content at that same position is an error. The
first answer wins within the transaction; later clients receive that answer.
The engine must validate the generation, deadline and selected option before
calling the answer operation. Permission and elicitation rows enter the
snapshot only while open, within the same read transaction as its cursor.
The existing `requests` section carries `{kind, request}` items, where kind
is `permission` or `elicitation` and request is the corresponding public type.

## Transcript and limits

Transcript sequence keys are eight-byte big-endian values. Their ordering
supports the full unsigned 64-bit range without SQLite's signed integer limit.
The engine allocates sequences monotonically per pane, across process
generations. A chunk update retains the message identity, role and kind;
completed content is immutable. Resume replay must reconcile existing content
before calling these writers, not append a second copy.

Each normalized record is at most 256 KiB of serialized UTF-8. A page has at
most 200 records and 512 KiB including its result envelope. The cursor is an
exclusive upper bound, and each page is returned in display order. Retention
removes whole oldest records until at most 32 MiB remain per pane. The sequence
allocator is stored in runtime state, not reconstructed from retained rows.

Schema 39 keeps each pane's transcript size as a running total (`agent_transcript_total`, one
row per pane and table), maintained by triggers in the transaction of every insert, update and
delete of `agent_transcript` and `agent_replay_transcript`, so no writer can skip it. A save
reads that row instead of summing the pane's messages, and the work of a save no longer grows
with the length of the conversation, nor does a replay of N messages cost N²/2 row visits.
`tests/agent_transcript_total.rs` checks the total against the sum of the rows after a random
sequence of writes, retention trims and replay publications, and that a save takes the same
number of SQLite steps with 20 and with 2,000 messages stored.

An engine of an earlier schema refuses a database of schema 39, so reverting the change that added
it must keep migration 039 and the schema version.

Runtime checkpoints are limited to 16 MiB. The native outbox holds at most
128 complete lines and 8 MiB per pane. Open native requests are limited to
128, with 16 MiB of combined private and public parameters; public request
metadata is at most 512 KiB so a snapshot item always fits one frame. These
are internal limits. Exceeding one fails the transaction; it never silently
truncates a prompt or acknowledges a partial effect.

Probe results are keyed by fingerprint and method. An advertisement does not
write a successful result. A new fingerprint cannot reuse old proof.

## Validation and integration boundary

Storage tests reopen the database across receipt and reply boundaries, force
a rollback after choosing a winner, replay a null-ID request, refuse delivery
to a changed process, and reject stale-generation mutations. Other tests
exercise pages across the signed integer boundary, byte budgets, whole-record
retention, completed-message integrity, cache invalidation and a populated
schema-3 migration. Process driving and the ten agent acceptance flows are
implemented in the following engine integration step.
