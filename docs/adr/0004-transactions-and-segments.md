# 0004 — Transactions and terminal segments

Status: accepted for implementation, 2026-09-11.

## Context

Contract 08 requires state, event and idempotent result to be acknowledged
together. Contract 07 requires writing output before the ack and recovering
the screen even when the shell process must be recreated.

## Decision

The engine is SQLite's only writer. Each mutation runs an IMMEDIATE transaction
with the state change, events and idempotent result. A repeated key in the same
scope returns the stored result; different content returns a conflict.
The snapshot reads state and cursor in the same transaction.

The database uses WAL and synchronous=FULL. Each version has a numbered migration;
before migrating an existing database, the engine backs it up with SQLite's API.
A newer database version is refused.

Terminal segments use records with big-endian type, offset and size.
A record never crosses a 512 KiB segment. The engine synchronizes the file
before acknowledging bytes to the supervisor. Retention removes entire old
segments to maintain the 32 MiB limit per pane. A gap is an explicit record
with the lost interval. An incomplete tail is detected during recovery.

Opening a log flushes its newest file and owes the flush of the older ones. A record read at
start may have been written and not flushed if the engine ended before its flush. A log written
only by flushed appends (an agent pane) can hold such a record only in its newest file, so that
file is flushed at once, before the pane's output can be acknowledged: one flush per pane at
start, not one per retained segment. A log written by `append_unsynced` (a terminal) can hold
one in any file, so the older files are owed (`Log::take_unsynced`), and the terminal path
flushes them off the Core lock before it acknowledges the pane's output. A file whose torn tail
was cut off is flushed once, as the newest. A flushed `append` flushes only the file it wrote.

The log belongs to the pane. A new shell receives a new generation and streamId;
replay of that stream includes the pane's previously recorded screen. Log offsets
keep increasing; the new process's sequence zero uses the previous log end as
its base. Restarting the process therefore neither erases the screen nor mixes
in-flight bytes from two generations.

## Rationale

A single transaction eliminates acknowledgements that exist only in memory.
The backup API includes the WAL. Separating process sequence and log offset
preserves the screen without pretending a process survived a machine restart.

Implementation reference: [backup API](https://docs.rs/rusqlite/latest/rusqlite/backup/index.html).
