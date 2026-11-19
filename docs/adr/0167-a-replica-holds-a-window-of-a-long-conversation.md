# ADR 0167: A replica holds a window of a long conversation

- Status: accepted. Extends ADR 0146 (each client keeps a domain in a replica): the agent replica holds
  a window of the transcript, not everything it ever received.
- Date: 2026-11-18

## Context

The engine keeps a conversation in SQLite only and pages it: `pane.transcript` returns the newest page
and, with `before`, the page older than a cursor (at most `transcriptPageMessages`). The clients did not
follow that: a replica (`AgentPaneView` in Swift, `AgentView` in the Windows and Linux client, which the
Android core shares) kept every message it was given and added a page of `pageSizeTranscript` on each
"Load earlier". It also kept the `content`, `rawInput` and `rawOutput` trees of every tool call, which no
view reads. Events are merged for every pane, shown or not, so the memory of a pane, and every walk over
its transcript (the grouping in the Swift view, `tail`, the ticker, the context ring), grew with how long
the pane had run. With 50 tabs of long conversations that is the cost the person pays, for panes they are
not looking at.

## Decision

- **Two numbers in `x-clientPolicy.caps`**, read through `ClientPolicy` and `soyeht_protocol::client_policy`:
  `heldMessages` (300) for a pane that is shown and `offscreenMessages` (50) for one that is not. 300 is
  three pages of the transcript: the 200 messages the Windows and Linux view draws at the newest, and one
  more page to read back into without asking the engine. 50 is half a page: a pane that is not shown draws
  nothing, and the header's ticker (the latest turn's tool calls and plan) and the first screen of the pane
  when it is shown again fit in it; the rest is one "Load earlier" away.
- **The rule is one function of the replica**, in `contracts/vectors/agent-transcript.json` (`window` and
  `cursor`), which both clients pass:
  - The newest `limit` messages are held, `limit` being one of the two numbers. When the person is at the
    newest message the older ones are let go of.
  - A person who has scrolled back keeps the half of a window before the message where they were, so
    nothing they can see is let go of and the window follows them. That holds for a pane that is not shown
    too: the place a person left a pane (`AgentTranscriptScroll.place`, the oldest message the desktop view
    draws) is where they come back to, so it is kept with the smaller window around it. What is read back
    to stays held until the person returns to the newest message.
  - Never let go of, in the latest turn (after the person's last message): a reply still streaming, a
    subagent that has not finished and a tool call that has not (the permission or question that waits on a
    tool call is read from the request the replica keeps apart, and what it points at in the transcript is
    that call). Anywhere: the newest usage report (the context ring reads it). What an earlier turn left
    unfinished is stale and goes with the rest, so a crash does not leave messages held for ever. A command's
    output is never final in a live update, so it holds nothing. A kept message older than the ones let go
    of is drawn with a gap below it; it is the accepted cost and a case of the vector.
  - A merge trims only a pane that grew past its limit and a page (`pageSizeTranscript`) more, back down to
    the limit, so a streaming pane does not rebuild its transcript for each message. A pane that leaves the
    screen is trimmed at once. Replacing a message already held trims nothing.
  - When messages are let go of, the cursor of "Load earlier" becomes the sequence of the first message
    kept after the last one let go of, so the engine's next page starts where the replica stops and
    anything let go of, a pinned message included, comes back through the same paging. A page of the newest
    messages (a turn ended, a pane shown again) does not move the cursor past what the replica does not
    hold: the cursor is the older of the one held and the page's.
  - A pane that is shown again holding less than a page, with more at the engine, reads the newest page
    once. A pane that is not shown does not read the transcript when a turn ends; the events it keeps
    receiving keep it current.
- **Payload is dropped on ingest.** A tool call keeps its title, kind, status and locations, which the
  views and the ticker read. `content`, `rawInput` and `rawOutput` are dropped when a message is merged,
  from an update or from a page, on every client. No view and no copy action reads them (Swift
  `AgentConversationCards`, the desktop `tool_row`, the Android item).
- **When the rule runs**: after every merge that adds or places a message, after a page, and when a pane
  stops being shown. The Swift conversation view reports that it is shown or not (`isVisible`, mounted or
  not), and where the person was (`AgentTranscriptScroll.place`). The Windows and Linux client takes a pane
  as shown when it is in the selected workspace, and as read back when its view is not at the newest; the
  view reports the oldest message it draws, so nothing drawn is let go of.
- **Not done here.** The window counts messages, not bytes: 300 messages of up to `transcriptMessageBytes`
  (256 KiB) are still up to 75 MiB for a pane that is shown. A bound in bytes is the next one. The wire is
  unchanged. A client that lets go of the newest end cannot get it back
  except through the newest page, so the window slides at the oldest end only; an `after` cursor on
  `pane.transcript` would let it slide both ways, and would let "Load earlier" become a fetch on scroll.

## Consequences

- What a pane holds is at most its window and a page, plus what is pinned and what the person has read back to. The
  walks over a transcript are bounded by it.
- A pane that is shown again after it was let go of reads the newest page and holds up to 300 again as
  messages arrive; the person who left it scrolled back finds the place they left.
- A replica built only from events (never loaded) is trimmed too; loading the pane replaces it with the
  newest page, and the cursor comes from that page.
