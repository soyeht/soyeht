# ADR 0040: Keep staged loop edits in the aligned panel

Date: 2026-09-12
Status: accepted for design alignment pass 1

## Context

The approved loop panel shows the loop configuration and a history affordance,
but it does not show how staged changes are committed or discarded. The shipped
Mac view keeps role and loop edits local until the person chooses Apply, and it
offers Cancel to discard them. Applying each field immediately would change the
interaction and could persist an incomplete prompt.

The approved panel also states that dispatch happens when the pane is idle. The
wire contract has no configurable trigger mode: idleness is a fixed engine rule.

## Decision

Keep the fixed idle trigger as explanatory text. Make the icon and title in the
panel header the Worker/Lead menu, without adding another visible control to the
approved header. Keep explicit Cancel and Apply actions in the footer. Match the
approved component in size, palette, type, spacing, wells, pills, toggle, and
placement. Replace the nonfunctional history affordance with the two existing
actions.

## Consequences

The panel remains truthful to engine behavior and preserves every shipped
control. Its footer differs from the approved frame by two short action labels.
No protocol, engine, store, or task-replica behavior changes.
