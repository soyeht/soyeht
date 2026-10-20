# ADR 0099: The iPad shows a workspace as a board of panes

- Status: accepted
- Date: 2026-10-19

## Context

At regular width the iPad app was a sidebar next to one pane. The Mac and the desktop client
show a workspace's panes together, arranged by the layout the Mac stores for the workspace
(`workspace.layout.set`, `PaneLayoutTree`), and fit that arrangement to the window with
`PaneLayoutFit` (the pane layout vectors in `contracts/vectors/pane-layout.json`). An iPad has the room for the same
view, and the person who arranged the panes on the Mac expects to find them in that order.

The paired device also followed exactly one terminal: focusing another pane unsubscribed the
previous one, and a terminal view took the client's single output sink.

## Decision

**One layout engine.** `PaneBoardPolicy` (`apps/shared`, `PaneBoard.swift`) fits the
workspace's presented tree to the detail area with `PaneLayoutFit`, with floors from
`PaneLayoutFloorFormula` and the fonts the app draws. It adds no arrangement rule of its own:
side by side when the stored arrangement fits, then at compact floors, then one row or one
column, then a tab group, as for the Mac. The stored layout is read through
`EngineClient.presentedLayoutTree`, the path the iPhone's writing view uses, so a pane the Mac
has not arranged yet joins the tree by the same insertion rule. A viewport smaller than one
pane scrolls the board.

**The board is for a wide layout.** It is used when the size class is regular and the
workspace has more than one pane to lay out and no writing pane. A compact layout (iPhone, a
narrow Split View or Stage Manager window) keeps the list and the one-pane navigation. A
workspace with a writing pane keeps the writing workspace, which arranges that pane and its
companion with `PhoneWritingLayoutPolicy`; merging the two arrangements is left for later.

**Read-only arrangement.** The board never writes the layout, not even to repair it. Dragging
dividers and moving panes stay on the Mac. What the iPad owns is focus.

**Focus follows the tap.** The focused pane is the client's selected pane (`select`), so the
engine's per-client focus and the keyboard follow it. A tap on a pane, or a terminal taking
the keyboard, focuses it. `PaneBoard` holds the keyboard rules: Command-] and Command-[ step
through the panes in visual order, Option-Command-arrow moves to the pane beside, and a tab
group answers with the tab shown.

**Several terminals at once.** The paired device used to follow one terminal: focusing
another pane unsubscribed the previous one. `TerminalPresenters` (`apps/shared`, the one
place for this rule) counts the terminal views that present each pane, by view. Each
`PhoneTerminal(paneId:)` claims the output of its own pane and registers when it appears.
- *Subscribing.* A view that registers while connected asks the engine for its pane's screen
  from the tail, even when a stream exists, because a new view starts empty (a card rebuilt by
  a rotation or a move between side by side and the tab group). The replay goes to the newest
  sink.
- *Ending.* A stream ends only when its last view goes away, so a card replaced before the old
  one is dismantled keeps it. It also does not end when a single-pane screen has taken the pane
  over, as when the board drops to one pane: that screen subscribed for itself.
- *Focus.* Changing the selection no longer unsubscribes the previous terminal while a view
  presents it (`unsubscribeAfterSelecting`).
- *Reconnect.* Leaving the app stops the connection, and the paired device then clears its
  panes and streams. After the next snapshot the device subscribes the selected terminal and
  every terminal a view presents (`subscribeShownTerminals`); a view that mounted before the
  connection came back asked for nothing, and one that mounts later asks for itself. Without
  this, the terminals that did not have the focus stayed silent after the app came back.
- Views are counted by owner, so two scenes that share one client need no per-scene
  bookkeeping: each view registers itself and a stream lasts while any view shows it.

The single-pane path is unchanged.

## Consequences

- The Mac and the iPad show a workspace in the same order, and a change of the layout on the
  Mac moves the iPad's panes through the existing `workspace.layout_updated` event.
- Every board terminal obeys its pane's size mode (`TerminalSizeModeStore`, ADR 0013 and
  #584). A pane opens following the Mac's size: its terminal reports
  nothing and takes the pane's grid, so opening a board does not reflow the Mac's terminals.
  A pane the person set to "Fit to this screen" reports the size of its cell on the board,
  again when the board lays out another way (rotation, stacking), and only that pane does.
  The engine is unchanged. The terminal menu with text size and pane size sits in each
  terminal's header on the board.
- A workspace that mixes a writing pane with terminals and agents does not get a board yet.
- Editor panes are not drawn on the iPad (a placeholder says so); that is a separate piece
  of work.
