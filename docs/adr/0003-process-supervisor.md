# 0003 — Process supervisor channel

Status: accepted for implementation, 2026-09-11.

## Context

Contract 07 requires processes independent of the engine, a file-backed registry,
bounded retention, read acknowledgement and a single connected engine.
It does not specify the internal channel format or sequence unit.

## Decision

The channel uses a private Unix socket and JSON frames with the same 1 MiB
limit as the common codec. The internal version is independent of the client version.
Each request receives a response. The engine fetches retained output in batches;
PTY reading runs on another thread and does not depend on those fetches.

`seq` is an exclusive byte offset: acknowledging 100 acknowledges bytes
before 100. Each process starts at zero. Batches carry the interval and
next position; a fetch before the retained start returns an explicit gap.
Retained output never exceeds 8 MiB per process.

The registry is atomically replaced and synchronized before acknowledging
spawn, close or a new engine generation. A closed spawn key stays closed
and does not create another process. The supervisor identifier changes at
each startup; processes from the previous startup are not adopted by PID.

Only one engine connection may execute commands. A higher generation invalidates
the previous connection; a lower one is refused. The same generation cannot take
over a live connection. A file lock prevents two supervisors in the profile.

## Rationale

Offsets allow retention to become segments without inventing another numbering
system. Batch fetching keeps process output independent of a slow client.
Instance identifiers prevent confusing a reused PID with a process from an
older pane.
