# ADR 0068: isolate agent polling failures by boundary

Status: accepted
Date: 2026-09-15

## Context

The engine polls live agent panes in stable identifier order. The polling loop previously
treated exactly two public error kinds as recoverable for one pane and returned every other
error from the whole cycle. A persistent fault in one pane therefore prevented every pane
ordered after it from being polled, even when those panes and their processes were healthy.
The public error kind describes what a client receives; it does not establish whether the
failure belongs to one pane or to infrastructure shared by all panes.

## Decision

Polling failures are classified by their failing boundary. A missing supervisor instance
keeps its existing reconciliation path. Storage durability, exhausted storage, supervisor
transport and supervisor generation failures are global: the engine stops that polling
cycle because it cannot safely commit or reconcile further pane state.

Every other error produced while polling an agent is owned by that pane. The engine marks
that pane dead through the existing durable `fail_agent` transaction, records the reason in
its transcript and attention list, retires its process, and continues to the next pane. If
the failure transaction or process retirement itself fails, that new error still stops the
cycle because the engine could not complete the durable isolation.

The classification uses concrete engine error boundaries rather than a list of public error
kinds. New protocol, decoding, native-message or pane-invariant errors therefore default to
pane isolation. A new shared-infrastructure error must be added explicitly to the global
classification with its reason.

## Consequences

One malformed or inconsistent agent session cannot starve an unrelated pane. Shared storage
or supervisor failures remain visible to the server loop instead of being misreported as a
single dead agent. A debug-only integration fault keeps the earlier pane failing while a
later pane serves a real prompt, so the regression proof depends on the same stable ordering
that made the original class intermittent in use.
