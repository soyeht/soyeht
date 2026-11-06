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
  client. The client hands it what the engine said (`load`, `apply(event)`) and, for work it joins callers to, the operation to run
  (`refresh(_:now:operation:)`); it keeps the answers, their order and the rules about when to ask again.
- **`EngineClient` is the facade that owns the wire**: the connection and its lifecycle, requests and
  mutations, the dispatch of events, the reset on a disconnect. One extension file per domain
  (`EngineClientUsage.swift`) is the only place that sends that domain's requests, and it passes the answers
  to the replica.
- **Views read the replica and call the client for actions**: `client.usage.readings`, not a copy of it on the
  client. A view that only reads a domain can take its replica as a plain parameter. No view gets a new
  observable environment object: the replica is a property of the client, and the Mac still creates the client once, in
  `SoyehtMacApp.swift` (`check_mac_environment.py`).
- **Rules are tested on the replica alone**, with no client and no socket. A replica never matches a
  wire failure or sends a request: the client's extension decides what a rate limit means and when to ask
  again, and the replica only keeps the in-flight work it is handed.
- **Every replica is listed once**, in `EngineClient.replicas` (in `ClientReplica.swift`), and conforms to the internal `ClientReplica`
  (`cancelInFlight()` on every disconnect, `reset()` on a paired device's). A disconnect walks the list, so a
  new domain cannot be left out of the cleanup in `EngineClient.swift`.
- **One domain per change**, the smallest first: usage, then tasks (with the loops and the attention items),
  then the others. The properties a domain had on the client are removed with it, not kept as forwards: a
  forward would keep the client's surface as wide as it was.

## Relation to the replicas that exist

The client already holds `EditorWorkspaceReplica` and `TerminalStreamReplica`, which are structs; `SnapshotAssembly`
is a struct for one snapshot; `TaskReplica.swift` is only an extension of `EngineClient` that changes the
client's own properties (the pattern this ADR replaces). A replica is:

- **a struct held by the client** (`internal(set) var`) when it is plain state that the client mutates and that
  has no work in flight of its own: the editor's tabs and trees, the terminal stream indexes. The client is
  its only owner and its rules are pure functions of the value. Those stay as they are.
- **an `@Observable` class owned by the client** when views read it, so that a view that only reads can take
  it as a plain parameter, or when it keeps work in flight (a refresh task, a retry) that a disconnect has to
  cancel. Usage is both. It is not passed through the environment.

A struct replica moves to a class only with its own domain's change, when it meets one of those two reasons.

## The plan for tasks

Tasks, with the loops and the attention items, is next. Its replica follows the same rules, and the two places
where the old code mixed the wire into the state are decided here so the next change does not invent them:

- **The snapshot needs the panes.** The attention items are filed by the workspace of their pane, so the
  replica loads from the decoded sections and the panes: `load(sections, panes:)`. It decodes nothing from the
  wire itself; the client passes the sections of `state.snapshot` and its pane list.
- **Events that need a follow-up request return it.** `taskMoved`, `taskClaimed`, `taskReleased`,
  `taskBlocked`, `taskUnblocked` and `taskUpdated` mean "read this task again". `apply(event)` returns the
  `(workspaceId, taskId)` pairs to refresh, or nothing, and the client sends the `task.show`
  (`scheduleTaskRefresh`). The replica never sends a request and never holds a closure that does.
- **The closure `refresh(operation:)` of the usage replica is not the pattern for this.** It is there because
  the replica keeps the in-flight refresh and joins callers to it. A refresh that an event causes is a return
  value for the client to act on, not an operation the replica runs.

## Consequences

- The client's surface shrinks by a domain each time, and a domain's rules can be read in one file.
- A view that reads a domain can take its replica as a plain parameter and be previewed or tested without a
  client. (Observation already tracked each property of the client separately, so this is not a gain in
  invalidation.) A test pins that `client.usage.readings` is observed through the client.
- Code that read `client.usageReadings` reads `client.usage.readings`.
