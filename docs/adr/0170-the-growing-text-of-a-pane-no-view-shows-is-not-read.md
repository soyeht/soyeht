# ADR 0170: The growing text of a pane no view shows is not read

- Status: accepted. Amends ADR 0167 (a replica holds a window of a long conversation): the sentence "the
  events it receives meanwhile keep the rest current" holds for everything except the growing text of an
  unfinished reply or thought.
- Date: 2026-11-19

## Context

A Release build of the Mac app with one workspace on screen and agents working in another kept the main
thread 55 to 60% busy with one hidden agent and 94% with three. Counting what the client does showed that the
cost is not per chunk. Decoding and merging an update of a hidden pane costs the main thread 0.05 to 0.3 ms
(1.7% busy for one agent at 83 updates a second, with no window in the run). What the views cost followed the
number of writes to what they read: the client held every pane in one observable array, and a hidden
agent's turn wrote it about six times (its state, which was four writes, and the pane the engine sends at the
start and the end of the turn), which ran the conversation of the pane on screen and the grid of its
workspace again each time. A stand-in for the window (the shared conversation view with 300 messages, a grid
that reads the workspace's panes, a tab strip that reads the attention summary) went from 0.8% busy at rest
to 10.5%, 30.9% and 52.3% with one, three and six hidden agents; with the narrow reads below and this
decision it stands at 2.2%, 2.7% to 2.9% and 3.1% to 3.2%. That run shows the mechanism; whether the real
app is fixed is for a run of the Release build to say.

## Decision

- **The growing text of a pane no view shows is not read.** An update of kind `agent_message_chunk` or
  `agent_thought_chunk` with `complete` false carries the whole text so far, and the newest one is all that
  ever mattered (`AgentPartialUpdates`). For a pane no view shows the client marks the pane stale (ADR
  0167: it reads the newest page once when it is shown) and goes on: no decoding, no write, nothing for a
  frame to wait for. The Windows and Linux client does the same for a pane of a workspace that is not
  selected, and asks for no frame for it. Every other update is applied as before: a finished message, a
  tool call, a subagent row, a plan, usage. A row that is still open keeps its place and its end, and the
  window of ADR 0167 trims what a hidden pane holds, so the page read on show reaches what is held and
  leaves no hole.
- **A pane's views follow that pane.** The client holds the panes through one store (`PaneStore`) read at
  the grain the views need: the whole list (`panes`), one pane (`pane(_:)`), the panes of a workspace
  (`panes(in:)`) or of a mode (`panes(mode:)`). A write changes the list and the pane, workspace and mode it
  concerns, so a view that shows one pane or one workspace does not run for another workspace's agent. A
  pane's state is written once. A closed pane leaves nothing in the store.
- **The attention summary is a value of its own.** The tab strip and the rail read it (`agentAttentionRequests`, `agentWaitingTotal`, `agentWorking` and `agentAttention(in:)`; ADR 0172); it
  is made again when a pane or a request changes, and its readers hear of it only when it differs.

## Consequences

- A hidden agent costs the main thread the events of its turn and the one reload when its pane is shown,
  not its streamed text.
- "Shown" is the state of a conversation view, not a permission to follow a pane. A consumer that is not a
  view and needs a pane's live text (the voice loop of ADR 0166, the relay of the future Apple Watch app)
  must not depend on it: it will need a follow handle of its own, separate from `agentPaneShown`. That handle
  is not built here.
- A reader that reads `panes` still follows every change of every pane; the views that show one pane or one
  workspace are the ones on the narrower reads. The iPhone's whole-list readers are left for a later change.
