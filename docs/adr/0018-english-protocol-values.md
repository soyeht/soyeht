# ADR 0018 · English protocol values and existing Dev databases

Status: accepted. Scope: English naming before slice 2 implementation.

## Decision

The canonical pane states are `starting`, `working`, `idle`,
`waiting_for_you`, `cancelling`, `no_signal`, `recovering`, `dead`, and
`unknown`. Probe results are `worked`, `unsupported`, `unauthenticated`, and
`inconclusive`. The schema, Rust/Swift/TypeScript types, vectors and contracts
use those names. Readers reject the previous spellings; they are not aliases.

The task contract uses `todo`, `in_progress`, `in_review`, `completed` and
`cancelled`; blocking kinds are `dependency`, `missing_information` and
`external_error`. These are documentation-only in this slice: no task table
or implementation exists yet. Subitems retain `open` and `done`.

`protocolVersion` remains 1 because no client has been released. Rebuild the
Dev clients and engine from the same revision; an old Dev binary is not a
compatible client for the renamed enums.

## Migration and preservation

SQLite `user_version` advances from 2 to 3. The existing backup procedure
copies the database, including the WAL, before one IMMEDIATE transaction
translates the protocol-owned JSON fields and then changes the version.
Version 0 and 1 databases pass through the same migration sequence.

The migration has an explicit old-to-new mapping in `english_values.rs`.
It updates `panes.payload.state`, including closed panes, and the corresponding
fields in `pane.created`, `pane.updated`, `pane.renamed` and
`pane.state_changed` events. Stored idempotency responses containing a pane
are updated too, so retrying an acknowledged rename returns a valid pane
without repeating the operation. Only the four exact engine-authored shell
phrases are translated. User text, nested extension data, settings, names,
transcripts, credentials and terminal segments are not rewritten.

Database identity, event IDs and cursor, timestamps, actors, process identity,
instance keys, generation, offsets, focus, idempotency keys and request
fingerprints are preserved. Invalid JSON aborts the transaction, retaining
version 2 and the original values. Reopening version 3 does not migrate again.
There is no probe-result storage in version 2; its new names apply to the
schema now and to the probe cache when that component is implemented.

Legacy literals remain only where needed to migrate old records or prove
that old wire values are rejected. The design mock is exempt from the
English-language rule and remains unchanged.

## Validation and rollback

A populated version-2 Dev fixture covers every pane state, closed panes,
all four affected event kinds and stored idempotency responses. It verifies
the backup, reopening, exact preservation of unrelated data and rollback
after malformed history. Shared vectors cover every new pane state and
probe result, plus rejection of every previous spelling.

A local service check also upgrades a populated isolated Dev profile while
its shell stays under the same supervisor. It checks the same PID, retained
terminal bytes, database identity and repeated mutation result across the
engine replacement. The normal launchd and native-client checks remain
required in CI.

A downgrade is explicit: stop the new engine, retain its data separately,
and restore the matching pre-migration backup with the old binaries. Never
run the old engine against version 3 or silently discard newer data.
Supervisor generation fencing still applies during manual recovery; restoring
a backup alone never authorizes evicting a newer supervisor connection.
