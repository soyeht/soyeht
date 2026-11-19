# ADR 0168: A replay is sent in slices, as the connection's queue drains

- Status: accepted. Extends ADR 0014 (the output queue of a connection is bounded, and its overflow closes
  the connection) and ADR 0079 (nothing blocks under the Core lock).
- Date: 2026-11-19

## Context

`pane.stream.subscribe` replays the tail of a terminal (up to 2 MiB, `tailBytes`) before its live output. The
engine put the whole tail, or a whole delta from an offset, into the connection's outbound queue when it pumped
the subscription, and the queue holds `connectionQueueBytes` (8 MiB) before the engine closes the connection
as a slow consumer. Three terminals fit. A client that subscribes six at once and is busy for a moment before
it reads (it parses the first replay, or it is a phone that just woke up) was closed, and so was any client
that did not pace its subscribes by waiting for `stream.replay_done`, as the Swift client does since
`TerminalReplayGate`. The people this product is for have 20 to 60 terminals open across their workspaces, with
long sessions, so the engine cannot rely on every client, old or new, to pace itself: no client may be
disconnected, or lose history, for how many terminals it subscribes or how fast.

## Decision

- **A replay is fed to the queue only as it drains, and the first is served to its end first.** Each
  connection keeps the subscriptions it is owed a replay for in the order they were made (`replay.rs`). A step
  sends one slice of 256 KiB (four frames of the largest payload) of the first, which stays first until its
  `stream.replay_done` is sent; then the next is served. A step is taken only while the queue holds 768 KiB or
  less, so a replay never takes more than 1 MiB of the queue; the rest is for what the client is owed live and
  for its replies. The wire is the same: the same frames, and for one terminal `stream.replay_start`, its bytes
  in order, `stream.replay_done`. Nothing of a later replay is sent before the earlier one is done.
- **Nothing polls for room.** When the queue is above the level, the replay asks the outbox
  (`Outbox::has_room_or_wake`) to raise the tick's kick once the writer has brought it down. The request and
  the writer's check are `SeqCst`: each side stores and then reads what the other writes, and with weaker
  orderings both can read the old value and the wake is lost until the tick's ceiling. A client that reads
  nothing leaves its replays waiting, at no cost and with no spinning, and the connection is closed only if what
  it is owed live passes the queue, which is the `slow_consumer` that stays.
- **A terminal that is caught up is not held behind the others.** Live output of a subscription whose replay is
  done is pumped as before, whatever the other replays are doing; only the level of the queue is shared, and at
  most 1 MiB of replay stands ahead of a live byte.
- **The disk is read off the Core lock.** A slice that the memory of the screen no longer holds (a delta from
  an old offset, or a tail of a pane that records its output) is read by a job on a thread of its own
  (`Log::reader`, which copies out of the log what the read needs), one at a time for a connection. The job's
  result is applied under the lock after it checks that the subscription is still the one that asked and has not
  moved. A segment that retention removed meanwhile is read again, from where the log now begins, and a disk
  that fails twice in a row ends the connection (`worker_failed`), whose client starts over. Slices from memory
  are a copy of at most 256 KiB under the lock.
- **Cost follows what is asked, not what is open.** The scheduler's work per pump is the number of slices it
  sends, bounded by the queue's room. It holds one slice of one connection at a time and never a tail: the bytes
  of the 64 replays of the measurement (125 MiB) were never in memory at once.

## Consequences

- A client may subscribe every terminal it has at once and read at its own pace; `TerminalReplayGate` is no
  longer needed for the connection's sake, and stays as the order the Mac draws the terminals in.
- Replays are served in the order they were subscribed, so a client that wants the terminals on screen first
  subscribes those first: the first is ready after its own bytes, not after everyone's. A round robin of
  slices was tried first and made every terminal ready last (the first `replay_done` came after about 80% of
  the time of all of them), and a terminal gets no live output until its replay is done.
- Live subscriptions that fall behind the memory of a screen are still read from the disk under the lock; they
  are rare (the memory holds the last 2 MiB), and a follow-up can move them to the same reader.
- The tail of a pane that does not record its output is what the screen keeps in memory: its last 1024 records
  or 2 MiB, whichever is less. Short reads from the pty make that less than the contract's 2 MiB.
