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
- **The grid reads where the panes go, a card reads its pane.** `outlinedPanes(in:)` returns
  `PaneOutline` values (the pane's id, workspace, mode, name and writing kind and section), so a view
  cannot draw from them what changes at every turn. It is told when a pane comes, goes, moves, changes
  its mode or its name, and `paneOutline(of:)` does the same for one pane (the window reads it for the
  selected pane). The layout code takes either a `Pane` or a `PaneOutline` (`PaneLayoutSource`). The card
  of a pane reads `pane(_:)` itself (`LivePane`), so a pane's own change runs its own card and no other.
- **A state the pane holds already is not written.** The engine sends the pane and then its state; the
  second is not a change for any view. The agent's queue and sign-in methods are written only when they
  differ.
- **An agent in a tab nobody has open is not drawn.** What the client holds stays: the transcript, the
  scroll reference, the typed draft and its attachments, the turn clock of the header (kept per pane in
  the client) and whether the composer had the keyboard, which goes back to it when the tab is opened
  and the pane is the selected one. What lives only in a view is lost, as it already is when the person
  changes workspace: an answer typed into a question and not sent, the rows opened in the conversation
  (subagent disclosure, tool rows), a dictation in flight, an open popover or the automation panel, and
  the pending "new messages" mark. Drawing the agent again also sends `pane.draft` with `active` false
  when the draft is not empty, as a workspace switch does. Other panes stay mounted: a terminal keeps
  its screen.

## Consequences

- On the VM, the main thread with 3 hidden agents streaming back to back went from 96% to 22%, and with
  6 from 96% to 34%. What remains is the tab of each workspace changing its working dot at each turn,
  which is a change of what the chrome shows. That dot also changes the width of the tab, which makes the
  top bar plan its strip again; a fixed slot for the dot is a follow-up.
- A view that draws a pane must read `pane(_:)`: an outline carries only where the pane is.
