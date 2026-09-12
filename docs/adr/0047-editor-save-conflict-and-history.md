# ADR 0047: Editor save, conflict, and history semantics

- Status: accepted
- Date: 2026-09-12

## Context

The editor contract requires a durable draft before autosave, conditional disk
updates, external-change detection, conflict preservation, attributed history,
and restart recovery. The wire does not define a separate conflict-resolution
method or an actor role for a write made outside the engine.

The event frame is limited to 1 MiB, while a file may contain more changed line
numbers than one event can safely carry.

## Decision

Every accepted `file.write` persists the complete draft and its authoritative
connection author before replying. The engine schedules one save 500 ms after
the most recent accepted edit. It revalidates the canonical path and compares
the current disk contents with the recorded base before an atomic same-directory
write. The temporary file is synchronized, renamed, and followed by a parent
directory synchronization.

The system save has a deterministic idempotency key derived from file and disk
revision. If the engine stops after the rename but before the database commit,
the next engine recognizes that disk equals the durable draft and finishes the
same version without losing or duplicating it.

The engine checks watched files at most every 250 ms. An external change with
no draft becomes the new buffer and one history version. Its author uses the
closed `human_local` role with client `filesystem` and display name `external
change`; this is a sentinel for an identity the engine cannot know. Changed
line identifiers are ordered and capped at the generated 2,000-line viewport
limit so the event remains bounded.

An external change while a draft exists stores the new disk contents separately
and raises `file.conflict`. Buffer and disk reads then expose the two versions.
The first later `file.write` is explicit resolution: it adopts the observed disk
version as the new base, preserves the edited buffer as the draft, and schedules
a conditional autosave. `file.revert` follows the same resolution rule.

Each completed engine or external write creates one attributed history version.
History remains bounded and paged by the store limits. Restoring a version
creates a new draft and therefore a new attributed version after autosave; it
does not rewrite or delete history.

## Consequences

No acknowledged draft depends on a pending file-system effect. A disk change is
never silently overwritten after the engine has observed it. File replacement
by another process in the final comparison-to-rename interval remains a local
file-system race; the engine detects the resulting contents on its next watch.

The sentinel author is distinguishable from a signed-in person or bound agent,
but it stays within the protocol's closed actor roles. A future protocol version
may add a system actor and a compressed changed-range representation.
