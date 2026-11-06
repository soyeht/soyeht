# ADR 0146: The shared client keeps each domain in a replica, and `EngineClient` owns the wire

- Status: accepted. Applies the layer rule that the Mac and iOS views are thin over `SoyehtClient`
  (`AGENTS.md`, layer 5).
- Date: 2026-11-06

## Context

`EngineClient` is one `@MainActor @Observable` class with about 45 stored properties and 35 extension files:
the connection, the panes, the tasks, the usage, the editor, the writing documents, the web panes, the agents.
Every Mac and iOS view takes the whole client. The rules of a domain (how a task event changes a task, how a
usage reading replaces another and which order they keep) were methods of extensions that changed the client's
own properties, so a test of a rule built a client, and a reader could not tell which properties a rule
touched.

## Decision

- **A domain's state and the rules that change it live in a replica**: a `@MainActor @Observable` class that
  the client owns (`client.usage` is a `UsageReplica`). A replica has no connection and no reference to the
  client. The client hands it what the engine said (`load`, `apply(event)`) and the request to run
  (`refresh(_:now:operation:)`); it keeps the answers, their order and the rules about when to ask again.
- **`EngineClient` is the facade that owns the wire**: the connection and its lifecycle, requests and
  mutations, the dispatch of events, the reset on a disconnect. One extension file per domain
  (`EngineClientUsage.swift`) is the only place that sends that domain's requests, and it passes the answers
  to the replica.
- **Views read the replica and call the client for actions**: `client.usage.readings`, not a copy of it on the
  client. A view that only reads a domain can take its replica. No view gets a new observable environment
  object: the replica is a property of the client, and the Mac still creates the client once, in
  `SoyehtMacApp.swift` (`check_mac_environment.py`).
- **Rules are tested on the replica alone**, with no client and no socket.
- **One domain per change**, the smallest first: usage, then tasks (with the loops and the attention items),
  then the others. The properties a domain had on the client are removed with it, not kept as forwards: a
  forward would keep the client's surface as wide as it was.

## Consequences

- The client's surface shrinks by a domain each time, and a domain's rules can be read in one file.
- An observation of `client.usage.readings` tracks the replica's property, so a reading changing does not
  invalidate a view that reads the panes.
- Code that read `client.usageReadings` reads `client.usage.readings`.
