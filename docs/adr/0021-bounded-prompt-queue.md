# ADR 0021 · Bounded prompt queue on the wire

Status: accepted. Scope: engine integration in slice 2.

Contract 08 requires a snapshot to carry queued prompt IDs and origins. Its
original generated `AgentSession.queue` also carried every prompt body. At
64 prompts of 64 KiB each, one pane could exceed the 1 MiB frame limit and
prevent reconnection even though each submitted prompt was valid.

`AgentSession.queue` now contains `QueuedPromptSummary` entries: identity,
origin, status, time and optional generation. Complete text stays durably in
the engine and is returned by `pane.queue.list`, not truncated to fit a pane.
That method accepts an optional cursor and limit, and returns `nextCursor`
when another page exists. Responses contain at most 64 prompts or 512 KiB.

The opaque cursor binds the queue revision and next index. Every change to
queue membership or order increments the revision. A cursor from a different
revision fails with `resync_required`; the client reloads the first page.
This prevents concurrent removal or dispatch from silently skipping a prompt.
Draft changes do not reorder the queue.

The public protocol version remains 1 because no agent-pane client has been
released. Rust, Swift and TypeScript types and shared vectors change together.
No method or event is renamed, and the complete accepted prompt is preserved.
