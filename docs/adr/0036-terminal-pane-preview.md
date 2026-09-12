# ADR 0036: Preview inactive terminal panes during design alignment

Status: accepted for design alignment pass 1 on 2026-09-12.

## Context

The approved workspace frame shows multiple terminal panes at once. The shared
client currently owns one terminal buffer and one stream position. Selecting a
terminal unsubscribes the previous stream and subscribes the selected pane.
Giving every pane an independent live terminal would change client behaviour
and stream ownership, which is outside this presentation-only pass.

## Decision

The selected terminal pane keeps the live `MacTerminal`. An inactive terminal
pane keeps the same approved card, header, spacing and surface treatment, but
its body shows the pane phrase and working directory already published by the
engine. Selecting it moves the existing live terminal subscription to that
pane. Agent panes continue to render their own published conversation state.

## Consequences

The workspace matches the approved pane structure without introducing another
stream subscription model. Two terminal panes cannot both show live bytes in
this pass. A later client objective may move terminal buffers and subscriptions
to pane-scoped hosts without changing this visual structure.
