# ADR 0022 · Publish recovered native history atomically

Status: accepted for slice 2.

An engine restart adopts the live supervised process. Its native request IDs,
pending prompt and byte offsets remain unchanged. Reinitializing that process
would create a second conversation and lose pending permission decisions.

A missing process in a reconciled supervisor registry is different. Finish the
old generation's requests and turn, retire only its bound output queue, then
reserve a new process identity and generation before spawning. Keep accepted
waiting prompts in their separate durable queue. Never replay an active prompt
automatically after an ambiguous interruption.

The new process initializes and loads the saved native session. Native history
has no stable message identifiers suitable for deduplicating normalized chunks.
During load, normalize the replay into a private SQLite staging table. Retain
at most 32 MiB of complete staged records. Publish that bounded history in the
same transaction as the successful session-start event. A client reloads the
transcript after that event. Queued prompts remain separate and are unaffected.

The existing published history stays readable until publication. If the engine
restarts during load, restore the parser checkpoint and staged records. If the
native process is lost, discard its private replay and start reconciliation
again. A failed load never publishes a partial replay. Staging requires up to
32 MiB of additional temporary storage besides the published history and raw
recovery journal; storage failure must remain visible and must not acknowledge
uncommitted data.

SQLite schema 5 adds the private replay table, private sign-in launch records,
and persistent agent attention items, with the normal pre-migration backup.
The wire protocol version remains 1. Public generation and session-start events
already express this recovery boundary.

Validation covers rollback during publication, database reopen with unpublished
records, supervisor loss followed by native load, preserved queued decisions,
and a recovered transcript without duplicated user or assistant messages.
