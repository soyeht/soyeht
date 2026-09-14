# ADR 0059: selected element prompt budget

Status: accepted
Date: 2026-09-13

## Context

A selection may contain 20 elements. Each element has structured metadata and a PNG crop. The public prompt text and the native harness message already have fixed byte limits. Applying only the per-image browser limit could build a valid wire request that the harness cannot accept as one message.

## Decision

The Mac host keeps each selected-element crop at or below 30 KiB and 640 pixels wide. The shared client caps the generated Markdown context at 32 KiB, keeps every selected element represented, and refuses a request whose final text exceeds the protocol prompt limit. It also refuses an attachment set whose base64 expansion plus the maximum prompt cannot fit in one native harness message.

The full typed package remains in client memory for the composer preview. Only its bounded Markdown representation and the ordered PNG blocks cross into the agent session. The selected page data is labelled as untrusted context.

## Consequences

Twenty-element selection remains possible without raising any slice 1–4 limit. A complex crop may be downscaled to meet the message budget. A request that cannot fit fails before any attachment frame is sent.
