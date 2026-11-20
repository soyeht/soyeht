# ADR 0172: The window follows what it shows of a pane, not the pane

- Status: accepted. Amends ADR 0170 (the growing text of a pane no view shows is not read): that
  decision stopped the text, and this one stops the rest of a hidden agent's turn from running the
  window.
- Date: 2026-11-20

## Context

With ADR 0170 in, a Release build on a Mac VM with 104 panes (24 agents, 8 workspaces) still kept the
main thread 70% busy with one hidden agent that sat in another tab of the workspace on screen, and
96% busy with three. The cost was not decode or apply. A turn of an agent writes about ten events
(the pane with its queue, the pane working, its state, the usage line, the pane idle, its state
again, the end of the turn), and each of them ran the whole window:

- the root of the window read the whole attention summary, which is made again each time an agent
  starts or stops working, so every turn of every agent ran the window, the workspace tabs, the rail
  and, through them, every card;
- the tab strip read that summary in its own body, so one workspace's agent ran every tab;
- the grid of the workspace on screen read the list of that workspace's panes, which is written for
  any change of any of them, and handed each card its pane by value, so an event of the agent in a
  tab nobody had open ran every card;
- an agent in a tab nobody had open stayed mounted, and its conversation, header and composer ran for
  each of its own events.

## Decision

- **The attention summary is read as the values the views show.** The requests (the dock badge and the
  notifications), the total and whether any agent works (the rail) and, for each workspace, its waiting
  count and whether one of its agents works (its tab) are values of their own, each written only when it
  differs. A workspace's tab reads its own value in its own body.
- **The grid reads where the panes go, a card reads its pane.** `outlinedPanes(in:)` is told when a pane
  comes, goes, moves, changes its mode or its name, and `paneOutline(of:)` does the same for one pane (the
  window reads it for the selected pane). The card of a pane reads `pane(_:)` itself, so a pane's own
  change runs its own card and no other.
- **A state the pane holds already is not written.** The engine sends the pane and then its state; the
  second is not a change for any view. The agent's queue and sign-in methods are written only when they
  differ.
- **An agent in a tab nobody has open is not drawn.** Its transcript, scroll and draft live in the
  client, so the tab shows it as it was when it is opened, as an agent in another workspace does. Other
  panes stay mounted: a terminal keeps its screen.

## Consequences

- On the VM, the main thread with 3 hidden agents streaming back to back went from 96% to 22%, and with
  6 from 96% to 34%. What remains is the tab of each workspace changing its working dot at each turn,
  which is a change of what the chrome shows.
- A view that draws a pane must read `pane(_:)`, not the values of `outlinedPanes(in:)`, which are as
  they were at the last change of the outline.
