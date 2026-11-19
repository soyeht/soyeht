# ADR 0168: A pane no view shows is not followed message by message

- Status: accepted. Extends ADR 0167 (a replica holds a window of a long conversation): the sentence
  "the events it receives meanwhile keep the rest current" no longer holds for the messages of a pane that
  is not shown.
- Date: 2026-11-19

## Context

A Release build of the Mac app with one workspace on screen and agents working in another was measured with
`sample`: one agent streaming replies kept the main thread 55 to 60% busy, three kept it at 94%. The cost
was read as "about 1.8 ms per streamed chunk". Counting what the client does showed otherwise. Decoding and
merging an update of a hidden pane costs the main thread 0.05 to 0.3 ms (1.7% busy for one agent at 83
updates a second, with no window in the run). The rest was the window running its views again, and it
followed the number of writes to what the views read, not the number of chunks: each turn of a hidden agent
wrote the one list of panes about six times (its state, and the pane the engine sends at the start and the
end of the turn), and every view that read that list, among them the conversation of the pane on screen
and the grid of its workspace, ran again each time. A run of the same client with a stand-in for the
window (the shared conversation view, a grid that reads the workspace's panes, a tab strip that reads the
attention summary) went from 0.8% busy at rest to 10.5%, 30.9% and 52.3% with one, three and six hidden
agents, and the rise followed the writes of the list (about 7 ms for each). With this decision the same run
stands at 2.2%, 2.7% to 2.9% and 3.1% to 3.2%.

## Decision

- **The update of a message of a pane no view shows is not read.** The client marks the pane stale (ADR
  0167: it reads the newest page once when it is shown) and goes on: no decoding, no write to the pane's
  replica, nothing for a frame to wait for. Which pane it names is the one field read, from the tree the
  reader has already parsed. The Windows and Linux client does the same for a pane of a workspace that is
  not selected, and asks for no frame for it.
- **A pane's views follow that pane.** The client holds the panes through one store (`PaneStore`) that is
  read at the grain the views need: the whole list (`panes`), one pane (`pane(_:)`), the panes of a
  workspace (`panes(in:)`) or of a mode (`panes(mode:)`). A write changes the list and the pane, workspace
  and mode it concerns, so a view that shows one pane, or one workspace, does not run for another
  workspace's agent. The state of a pane is written once (it was four writes).
- **The attention summary is a value of its own.** The tab strip and the rail read it
  (`agentAttention`); it is made again when a pane or a request changes, and its readers hear of it only
  when it is not what it was.

## Consequences

- A hidden agent costs the main thread the events of its turn (a few a turn) and the one reload when its
  pane is shown, not its replies. A pane shown in the middle of a reply is stale, so it reads the page that
  holds the reply so far and then follows the live updates.
- A client that wants a pane's messages live must show the pane (the conversation view does, mounted or
  not). Nothing else in the clients reads the messages of a pane that no view shows.
- A reader that reads `panes` still follows every change of every pane; the views that show one pane or one
  workspace are the ones moved to the narrower reads.
