# ADR 0042 · Editor and usage subsystem boundaries

Date: 2026-09-12
Status: accepted for slice 4

## Context

Slice 4 adds a native editor, file ownership, language-server integration,
version history, source-control views, subscription-usage readers, and a Mac
app rail. These concerns have different lifetimes and failure modes. Leaving
their boundaries to emerge during implementation would make the engine own
buffer algorithms, process protocols, filesystem details, provider-specific
parsers, and presentation state.

The existing hardening rules remain mandatory: external effects use ports,
rules run against doubles, SQL stays in the store, each crate exposes one
named error enum, production functions stay below 120 lines, and wire types
come from the protocol schema.

## Decision

### Crates and dependency direction

Four independent Rust subsystems become workspace crates:

- `soyeht-editor` owns the piece-table buffer, edits, line index, viewport
  projection, incremental syntax tree, highlight spans, cursor operations,
  editor commands, and pure search operations. It performs no file, process,
  database, network, clock, or source-control IO.
- `soyeht-lsp` owns LSP framing, request correlation, restart-safe client
  state, language configuration, and mapping from LSP responses to its own
  typed diagnostics, symbols, locations, and edits. It defines a transport
  port and has no knowledge of panes, tasks, SQLite, or SwiftUI.
- `soyeht-usage` owns the ordered reader registry, provider reader trait,
  normalized reading model, refresh policy, stale and backoff rules, and one
  provider module per implemented reader. It defines typed source ports. A
  source adapter returns account facts or response bytes and never exposes a
  credential to the engine or store.
- `soyeht-vcs` owns repository discovery, status, diff, hunk application, and
  blame mapping. It defines a command port and returns typed paths, hunks, and
  line attribution. It does not read editor buffers or mutate task state.

These crates may depend on small third-party libraries and on
`soyeht-protocol` only when a generated closed enum is the shared domain
vocabulary. They never depend on `soyeht-engine`, `soyeht-store`, or
`soyeht-ptyd`. `soyeht-engine` is the composition root and maps subsystem
types to generated wire types. This keeps the dependency graph acyclic.

`soyeht-store` remains the only SQLite owner. It adds named operations for
drafts, file versions, file ownership metadata, and last usage readings. Each
operation has a real-database test, including migration and cap enforcement.
It does not watch files, run readers, interpret buffers, or publish events.

`soyeht-ptyd` remains the process owner. A language server is another durable
pipe process with generation, sequence, acknowledgement, retention, and
reconnection. The supervisor does not parse LSP or choose a language.

### Engine modules and state ownership

`soyeht-engine/src/files/` is the sole owner of an open file. It coordinates
`soyeht-editor`, the store, the disk and watcher ports, the LSP registry, and
the source-control port. It owns file identity, canonical workspace-relative
paths, buffer revision, disk revision, draft revision, author attribution,
the 500 ms save deadline, conflict state, and publication order.

The file owner uses two narrow external ports:

- `FileSystemPort` canonicalizes, reads, writes atomically, reports metadata,
  and enumerates/searches project files.
- `FileWatchPort` reports disk changes with an opaque watch generation.

Real adapters use the host filesystem. Test doubles keep bytes and watch
events in memory. Rules compare revisions before any write. A disk change and
a local edit can therefore produce a typed conflict without either side being
silently overwritten. Durable store changes commit before public events.

`soyeht-engine/src/languages/` assigns files to one language-server process
per language and adapts the existing `SupervisorPort` to the transport port
defined by `soyeht-lsp`. The engine owns restart and pane-facing status. The
LSP crate owns message framing and request state. Probes and CI use the
deterministic language-server peer in the repository.

`soyeht-engine/src/usage/` schedules the registry from `soyeht-usage`, applies
the 30-second manual-refresh limit, and publishes generated protocol results
and events. It stores only normalized readings. Real source adapters may read
an existing local account source or contact that provider's own endpoint.
They do not create, refresh, rotate, return, log, or persist credentials.
Scripted readers cover every public state in CI.

`soyeht-engine/src/editor_review/` coordinates agent hunk attribution,
acceptance, rejection, live cursor publication, and task feedback. It uses
typed editor revisions and VCS hunks. Only this engine module may translate a
line comment into a durable task change request.

### Client boundaries

Generated protocol types feed `apps/shared`. Shared replicas hold only facts
published by the engine: open-file revisions, viewport payloads, diagnostics,
review marks, usage readings, and LSP status. They do not derive authoritative
file, task, or usage state.

The Mac app owns presentation and local navigation:

- `EditorSurface` draws only the visible viewport and forwards edits and
  commands to the engine. It does not read or write workspace files directly.
- `AppRail` owns selected view, hover state, and the per-client rail-open
  preference. Switching views never issues a pane mutation.
- `UsageRings` renders shared usage facts and requests refreshes. It does not
  invoke a reader or read an account source.

`MacTheme` remains the only Mac source for color, type, spacing, radius, and
elevation. Screen-specific files may compose these tokens but may not define
parallel style constants. The existing text-font gate covers every new label.

### Source of truth and error mapping

`contracts/protocol.json` is the sole wire contract. Generation produces Rust,
Swift, and TypeScript methods, events, fields, and closed enums. Handwritten
code may adapt generated values but may not duplicate their string literals.

Each new crate exposes one `Error` enum with named variants. The engine maps
each subsystem error to its own engine error once, and the existing engine
wire mapper remains the only JSON-RPC error mapping. Raw subprocess, IO,
parser, and database errors never cross the wire.

### Verification boundaries

- `soyeht-editor` unit and performance tests use in-memory text and assert
  bounded viewport work and incremental reparsing.
- File-owner rule tests use memory file, watcher, store, clock, LSP, and VCS
  ports. Real-database tests remain in `soyeht-store`.
- `soyeht-lsp` tests use an in-memory transport; launchd end-to-end tests use
  the deterministic supervised peer.
- `soyeht-usage` tests use scripted source ports; real-login investigation is
  local, read-only, and records only normalized results and source names.
- `soyeht-vcs` rule tests use a fake command port; integration tests use a
  temporary repository.
- Mac view tests assert that rail switching has no engine mutation, viewport
  rendering stays bounded, and usage state is rendered without recomputation.

## Consequences

The engine coordinates durable facts and event order without becoming the
implementation site for text storage, LSP framing, provider parsing, or git
algorithms. The new crates can be tested without launchd, SQLite, sockets, or
the Mac UI. Real adapters remain small and are exercised separately.

This split adds explicit adapters and type conversions. That cost is accepted
because process failure, disk races, provider access, and high-frequency
editor input need independent tests and failure handling. A new provider,
language, or rendering surface extends a registry or adapter without changing
another subsystem's internals.
