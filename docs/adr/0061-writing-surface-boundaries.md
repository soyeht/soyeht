# ADR 0061: Keep writing surfaces native and reuse owned files

Status: accepted · 2026-09-14

## Context

Slice 6 adds text, presentation, screenplay, and beat-board surfaces over local
documents. These surfaces need richer presentation than the code editor while
preserving its file ownership, conflict, history, formatting, and agent-review
guarantees. The rendering spike compared native TextKit with an embedded web
editor on the required hybrid preview, suggestion, image, and large-document
paths.

## Decision

### Client rendering

The Mac client renders writing surfaces with TextKit and AppKit. The measured
probe in `docs/reports/prose-rendering-spike.md` selected this path. WebKit is
not part of the writing surface, and WebKit imports remain inside the bounded
browser module.

The client owns prose layout, inline Markdown presentation, slide layout,
screenplay typography, focus presentation, hover controls, and the beat-board
view. The engine never renders prose.

### Wire and file ownership

The protocol gains one additive `writing` pane mode and a closed writing kind:
`text`, `presentation`, or `screenplay`. Presentation is another view of a
Markdown document. The beat board is a view attached to a screenplay document,
not another pane mode.

Every writing pane is backed by the existing owned-file path. Opening, editing,
saving, reloading, conflicts, and history keep the guarantees of ADR 0046 and
ADR 0047. Formatting mutations use the durable revision and idempotency model
from ADR 0050. Agent rewrites and comments use the bounded change-request path
from ADR 0052. No second file writer is introduced.

### Pure core

A new `soyeht-writing` crate contains bounded, deterministic modules for:

- Markdown toolbar transformations and slide segmentation;
- Fountain parsing, ordered elements and scenes (the shared Swift client builds the
  SmartType suggestions);
- ordered per-line variants with a selected index;
- beat cards and their scene links;
- three disjoint document sections and their relative paths.

The crate depends only on `soyeht-protocol`, serialization, and small parsing
libraries. It never depends on the engine, store, supervisor, or a client. It
has one typed error enum. The engine maps that error once to the wire.

### Effects and ports

The engine remains the composition root. Directory enumeration, owned-file
reads and writes, image persistence, and sidecar persistence are effects behind
typed ports with in-memory doubles. The store remains the only SQLite owner.

Writing section roots are profile configuration owned by the engine. The three
roots are distinct, and the section core validates every relative path before
an effect runs. All disk paths are re-resolved through the existing canonical
workspace boundary before each read or write.

### Sidecar state

The canonical Markdown or Fountain file contains the writer-visible content.
Bounded structured state that the plain format cannot represent lives in one
adjacent sidecar named by the document identity. It contains screenplay line
alternates, the selected variant identity, beat cards, scene links, and optional
scene media prompts. The selected screenplay line is the only variant written
to canonical Fountain.

The engine journals a canonical-file and sidecar update before either disk
effect, re-validates both paths, and completes the operation idempotently. A
restart resumes the same journal. The agent receives both canonical content and
the bounded sidecar through its pane-bound tools; it cannot choose a variant on
behalf of the writer.

### Bounds

Protocol limits are the source of truth for document bytes, viewport lines,
slides, scenes, beat cards, links, variants per line, variant bytes, suggestions,
section entries, and pasted images. Pure cores reject work above those limits.
The engine re-checks the serialized response and every effect input. Clients
render only bounded results.

### Style and fonts

Writing views use `MacTheme` and `SoyehtThemeColors`. The serif reading face and
screenplay Courier face are registered as bundled, licensed resources and are
added to the font gate in the same changes that first use them. No view owns a
private colour or unregistered font.

## Consequences

The writing editor shares the mature file and agent lifecycle while keeping
document presentation local to the Mac. One pane mode limits wire growth, and
closed writing kinds keep the three sections disjoint. A single pure crate can
test document semantics without disk, process, socket, database, or UI.

Sidecar changes require a durable two-file journal because canonical Fountain
and its structured alternates must never silently diverge. The journal adds
engine and store work, but it makes restart and retry behavior explicit.

