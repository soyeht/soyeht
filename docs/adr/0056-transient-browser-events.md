# ADR 0056 · Transient browser events have no state cursor

Date: 2026-09-13  
Status: accepted for slice 5

## Context

The executable protocol marked local-server changes, web console counts and
web driving state as non-durable, but their parameter types still required
`eventId` and `at`. A client advances its replay cursor when `eventId` is
present. Persisting these notifications would contradict the non-durable flag;
using a synthetic cursor could make a client discard a valid notification or
skip a durable event.

These facts are temporary. Local servers are rediscovered, console counts come
from an in-memory ring, and driving state ends with the routed command or host
connection. Only navigation changes the durable web-pane record.

## Decision

`servers.local.changed`, `web.console_counts` and `web.agent_driving` carry no
`eventId`, `at` or actor. The engine publishes them directly to live subscribed
clients and never inserts them into the event log. The generated event registry
must identify the name as non-durable before the engine can use this path.

`web.navigated` remains durable. Its URL, title and navigation generation are
written with the pane record and event in one transaction. A reconnect obtains
that state from the snapshot and then resumes durable events from its cursor.

## Consequences

Transient notifications cannot create gaps in replay or move the state cursor.
A reconnect starts with no console counts and no active-driving badge until the
live host publishes fresh facts. Local servers are refreshed on the existing
five-second schedule. Protocol version remains 1 because no released client has
consumed the slice-5 event shapes.
