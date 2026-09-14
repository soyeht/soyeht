# ADR 0063: Bounded writing asset reads

Date: 2026-09-14
Status: Accepted

## Context

Presentation Markdown refers to content-addressed PNG files beside the owned document. The Mac surface must render those images, but ADR 0061 keeps workspace file I/O in the engine. JSON cannot carry a binary image within the one MiB message budget.

## Decision

The additive `writing.asset.read` method accepts an owned `fileId`, its current revision, and an asset identifier. The identifier is limited to the exact `assets/<64 lowercase hexadecimal characters>.png` form during engine handling. The engine resolves that path relative to the document, revalidates canonical workspace containment, accepts only a regular non-symlink PNG no larger than `WRITING_IMAGE_BYTES`, and revision-checks before and after the read.

The result contains an `AttachmentRef`. Type-3 attachment frames precede the JSON response on the same ordered outbox. The client assembles the declared bytes before resolving the pending typed request and rejects missing, extra, mismatched, or over-budget attachment data.

Protocol version 1 remains unchanged because no released client exists and the operation is additive.

## Consequences

The client renders the asset without acquiring filesystem authority. A stale document revision, changed asset, malformed identifier, scope escape, or incomplete binary transfer fails closed. The general response attachment path is reusable for bounded engine-owned binary results, while this method exposes only immutable presentation assets.
