# ADR 0049: Durable project replacement

Date: 2026-09-12
Status: accepted for slice 4

## Context

`file.search` can replace text in several workspace files. A file-system write
cannot share the SQLite transaction that records history and the public
idempotency receipt. The engine can therefore stop after renaming a file but
before recording that write. Project replacement also reaches files that do
not have an editor pane, while contract 09 requires local history for every
file the engine writes.

## Decision

An editor file has a durable workspace-and-path identity independent of an
open pane. `paneId` is optional in storage and remains required on the public
`FileDocument`; detached records never enter snapshots. Opening a detached
record attaches its existing identity and history to the new pane. Closing or
deleting a pane does not delete file history.

A replacement is a durable journal scoped to one workspace. Planning records
the request fingerprint, normalized author, bounded public matches, total
replacement count, and one ordered item per affected file. Each item records
the opaque file identity plus hashes of the expected and resulting content.
Only one unfinished replacement may exist in a workspace.

The engine refuses project replacement while any affected open file has a
draft or conflict. For each clean item it resolves the canonical path again
immediately before a conditional write. If disk content has the expected hash,
the engine writes the planned result. If it already has the resulting hash,
the engine treats this as recovery after a completed rename. Any other content
stops the operation without overwriting the external change.

After a write, one SQLite transaction advances the file revision, records the
attributed history version, and marks the journal item applied. The public
idempotency receipt is written only after every item is applied. Retrying the
same request resumes the journal; a different request cannot take over its key
or workspace. Replacement is atomic per file and recoverable as one operation,
but it does not claim an atomic rename across several files.

## Consequences

An engine stop cannot cause the same replacement to be applied twice. A
completed disk write is recoverable without losing history, and unopened files
gain history without fake panes. A concurrent external edit fails closed and
requires the person to inspect the affected workspace before retrying.
