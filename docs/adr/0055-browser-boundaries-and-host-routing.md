# ADR 0055 · Browser boundaries and host routing

Date: 2026-09-13  
Status: accepted for slice 5

## Context

Slice 5 adds a visible web pane, local-server discovery, element packaging,
agent actions, session permission, and bounded console and network views. The
page runs in WebKit in the Mac client. The engine remains the authority for
pane identity, durable metadata, permissions, events, and agent-tool access.

The human-mode spike found that process-directed events do not reach an
out-of-process WebKit page. System-wide events reach the page as trusted input
but take over the person's pointer. The selected fallback is a virtual machine
with its own browser and input device. Human mode is therefore disabled in the
host Mac web pane and is not part of the host implementation in this slice.

The contract names `web.command` as an engine-to-client request but does not
specify how a reconnecting client claims the host role or how responses are
correlated. This ADR closes those implementation boundaries without moving
page state into the engine.

## Decision

### Pure web core

`soyeht-web` is a new pure Rust crate. It depends only on `soyeht-protocol` and
small parsing libraries. It never depends on the engine, store, supervisor, or
a platform web view. Its modules own:

- URL normalization and the closed `http` and `https` scheme policy;
- parsing and normalization of listening-port observations;
- bounded accessibility snapshots and navigation-scoped element references;
- bounded pick-element packages;
- bounded console and network metadata rings;
- command validation, timeouts, viewport presets, and response budgets.

The crate exposes one `Error` enum. The engine maps it once into the engine
error enum, whose existing wire mapper remains the sole JSON-RPC mapping.

The engine supplies a `PortScanPort` with a real `lsof` adapter and an in-memory
double. The adapter returns bounded observation bytes. `soyeht-web` parses the
bytes into typed facts and caps the public list at 50 entries. The engine owns
the five-second refresh schedule and publishes `servers.local.changed` only
when normalized facts change.

### Engine ownership and host routing

`soyeht-engine/src/web/` owns web-pane records, the ten-pane workspace limit,
host selection, command correlation, permissions, idempotency, public events,
and mapping between generated wire types and `soyeht-web` types. It never
loads, renders, scripts, screenshots, or stores a page.

The client handshake gains an optional generated capability list. The Mac app
advertises `web_host`; CLI, MCP, phone, and older clients do not. At most one
live local-human connection may hold that capability in v1. A second claim is
rejected with `web_host_unavailable` until the first connection closes; the
client then connects without the claim instead of retrying it. This avoids selecting a host by a
free-form client-name string.

The live host connection owns every web pane for that engine. On reconnect it
receives the durable pane records in the ordinary snapshot, recreates the
views, and becomes their host after the normal welcome. When no capable host
is connected, web tools fail with `WEB_HOST_UNAVAILABLE`. Host identity is
transient and is never stored in SQLite.

`web.command` is a JSON-RPC request in the reverse direction. The engine
allocates a command id, records a bounded in-memory waiter containing the
originating connection and request id, sends the request through the host
outbox, and completes the original request only after the typed host response.
The host response uses the same JSON-RPC envelope and limits. Unknown, late,
duplicate, and mismatched response ids are rejected. Disconnect fails every
waiter owned by that host. The caller may retry its original mutation with the
same idempotency key; durable engine receipts prevent duplicate pane changes.

No command waiter survives an engine restart. This is intentional: page
actions cannot be claimed complete without a host response. A disconnected
caller reconnects and retries. Read commands have no durable effect; mutation
commands carry an engine-issued operation key so the host can suppress a
duplicate within the life of the reconstructed pane.

### Persistence and privacy

`soyeht-store` remains the only SQLite owner. A web pane persists only its URL,
title, viewport, incognito flag, and ordinary pane identity. Navigation
history, element references, screenshots, console entries, network entries,
cookies, host connection ids, and command waiters are never stored.

The Mac app owns one persistent `WKWebsiteDataStore` for each Soyeht profile.
Incognito panes use a non-persistent store. The engine stores durable grants
for `web_session` by workspace and domain, but never receives cookie names or
values. Before the first agent action that could send cookies, the host returns
only whether the target domain has cookies. The engine then requires a grant
or creates the existing durable permission flow. No agent permission mode
answers this permission (ADR 0070). Denial does not clear the user's session.

### Mac WebKit boundary

`apps/mac/Sources/Web/` is the only module that imports WebKit. It is split
into the visible pane, navigation delegate, command executor, element picker,
snapshot assembly, metadata collection, and website-data creation. Pure cadence and
selection logic lives in files that do not import WebKit and has direct unit
tests. The shared client holds typed replicas and reverse-request correlation;
it never renders or evaluates a page.

The WebKit module enforces the scheme policy again before navigation. It uses
only generated command and result types. It keeps console and network rings in
memory at 500 and 300 entries, strips headers and bodies before insertion, and
rechecks every payload budget before returning it. Element references carry a
navigation generation and expire atomically on provisional navigation.

All controls, empty states, picker overlays, badges, and status text use
`MacTheme` and `SoyehtThemeColors`. The existing font and color gates cover the
new files. The Mac app consumes Screens 13, 13b and 13c without changing the
design file.

### Agent tools and authority

`soyeht-mcp` exposes the closed `web_*` tool set and forwards typed requests to
the engine. It has no direct connection to WebKit. The engine checks the MCP
binding, workspace visibility, pane identity, mode, host availability,
session grant, takeover state, reference generation, and method-specific
permission before routing a command.

Snapshot mode may use structured page data. Human mode exposes only
screenshots and coordinate actions, but remains unavailable on the host after
the spike. `web_evaluate` always creates its own permission request and is
never answered by an agent permission mode. Console and network results expose metadata only.

### Limits and verification

The protocol schema is the sole wire source and remains version 1. Changes are
additive. Every public response is checked against both its semantic cap and
the frame cap: 10 web panes per workspace, 200 KiB snapshots, 2 MiB
screenshots, 64 KiB evaluation results, 500 console entries, 300 network
entries, 60-second waits, 50 local servers, and 20 picked elements.

Pure-core tests cover normalization, scheme refusal, navigation generations,
ring eviction, redaction, selector/package limits, and exact boundary values.
Engine rule tests use memory host, store, clock, and port-scan doubles. Store
tests use a real database. Shared-client tests exercise reverse JSON-RPC
requests and reconnect. Mac tests use a deterministic local page and a WebKit
host double where pixel rendering is not required. End-to-end tests run the
real Dev engine under launchd with a deterministic fake web host.

## Consequences

The engine can authorize and audit agent web actions without becoming a
browser. Page data and private session material remain in the client process.
Reverse requests add a bounded bidirectional JSON-RPC state machine, but do
not create a second protocol or a durable action queue.

The host Mac implementation ships structured browser tools and ordinary user
navigation. It does not claim that trusted background input can coexist with
an independent user pointer. Human mode returns only behind a separate virtual
machine boundary and a green repeat of the spike, including the external
signed-in-site check.
