# ADR 0062: Reserve writing document identity before creating its file

Status: accepted · 2026-09-14

## Context

`writing.document.create` must create a local file, an owned editor file, a
writing record, and a pane as one recoverable operation. The filesystem and
SQLite cannot share one transaction. A crash between those effects must not
create a second file or identity when the client retries.

Document names also need one deterministic mapping into the three section
folders. Silent suffixes would make a retry ambiguous and could hide an
existing document from the writer.

## Decision

The engine derives one normalized relative path from the requested section,
optional subfolder, and title. The title becomes a lowercase, hyphen-separated
filename with a bounded stem. Text and presentation documents use `.md`;
screenplays use `.fountain`. An existing target is refused. The engine never
adds an implicit numeric suffix.

Before touching the filesystem, the store records a creation intent keyed by
the caller scope and idempotency key. The intent contains the request
fingerprint and stable operation, file, and pane identities. Reusing the key
with different parameters fails with `idempotency_conflict`.

The filesystem port creates missing section directories one component at a
time. Every existing component must be a real directory, each canonical path
must remain inside the workspace, and the final file is opened with exclusive
creation. The file is synchronized before the operation continues.

If the engine restarts after creating the file, the durable intent permits one
narrow recovery: the same regular, empty file may be adopted. A symbolic link,
directory, or nonempty file is refused without modification. A request without
the matching durable intent never adopts an existing file.

After the disk effect succeeds, one store transaction creates the pane, owned
editor file, writing document, idempotency receipt, and events, and removes the
intent. The operation then installs the already-persisted state in memory.

## Consequences

A retry returns the same file and pane identities across process restarts.
Creating a document cannot overwrite or silently rename existing work, and
directory links cannot escape the workspace boundary. An interrupted creation
may leave an empty file and a durable intent, but the next identical request
finishes it safely.

The operation intentionally does not write starter prose. Initial templates
can be a later, revision-checked editor write after creation rather than part of
the cross-resource commit.
