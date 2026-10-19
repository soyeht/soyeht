# ADR 0095: A slow read or a slow disk does not hold the engine

- Status: accepted
- Date: 2026-10-18

## Context

Clicking a file in the sidebar took seconds on a busy Mac. The engine log of that session
showed three things at once:

- `file.diff` ran for 6.4 s (five `git` processes on a cold repository) and the `file.open`
  and `file.read` of the next clicks started only when it finished. ADR 0079 says an offloaded
  request "keeps its place in its connection's order": the connection read loop waited for the
  work before it read the next frame. The editor asks for a diff after every open, so a slow
  `git` delayed the text of the next file.
- The tick held the Core lock for 1.6 to 4.4 s (`engine.core_lock_slow purpose=tick`). Every
  shell pane's output was written to its segment file and flushed to the disk (`File::sync_all`,
  a full flush of the drive on macOS) inside the tick, before the supervisor was told it could
  drop the output. With many panes that is one flush per pane per tick, 100 to 300 ms when
  the disk is idle and seconds when the machine writes a lot.
- `file.diff` failed with `internal_error` in a folder that is not a repository, and every
  `file.read` of a large file copied the whole text twice and built it a third time to compare.

## Decision

**A read may overtake the requests behind it.** `Offload::concurrent()` marks a request whose
work runs while the connection goes on reading: the reply is sent when the work ends, and the
client matches it by id. It is set only for reads no later request depends on: `file.diff`,
`file.search` (without a replacement) and `servers.local.list`. Every other offloaded request
keeps its place in the connection's order, as ADR 0079 says. A connection has at most
`CONCURRENT_OFFLOADS` of them in progress and then waits for one to end.

**The tick never waits for the disk.** The terminal path writes the output to the segment file
(`Log::append_unsynced`) and goes on; the output is readable at once. The flush runs as a
background job (`Screen::take_durable`, `Core::acknowledge_when_durable`), one at a time per
pane, and the acknowledgement to the supervisor follows it. The supervisor keeps the output
until then, so a crash before the flush loses nothing: the engine drains it again from its
last durable position. A flush that fails is owed again by the next job
(`Log::restore_unsynced`), and so is an acknowledgement the supervisor refused after a successful flush;
a job that cannot start gives the owed flush back and logs it, and never stops the drain of the other panes. Creating a segment
and removing the oldest one owe the flush of the directory with the records, so a rotation does not wait for the disk either.
A pane that is not recorded is acknowledged at once. Agent panes
and the other callers of `Screen::append` still flush before they return.

**A folder that is not a repository has an empty diff.** `file.diff` answers with no hunks and
no blame, without starting `git`, when no folder above the workspace holds a `.git` entry. It
is not an error and needs no error code.

**A read of the buffer copies nothing.** `file.read` reads the lines from the document the
editor already holds (`EditorService::viewport_of_buffer`), compares it with the text through
`TextBuffer::matches` instead of building a copy, and describes the file with
`document(&EditorFile)` instead of a clone.

**The client reads the diff after the text, once per revision.** `EngineClient.loadEditorDiff`
asks only when the first page is held, after the revision has been still for a moment, and never
twice for one revision, so appearing again or switching tabs asks for nothing.

## Consequences

- Terminal output of every pane and every request keep flowing while a disk flush or a `git`
  command takes seconds. `tests/slow_disk.rs` and `tests/offload_order.rs` fail on the old code.
- The flush shares the four background workers with the other jobs. A flush that waits behind
  slow jobs delays only the acknowledgement; the supervisor keeps its output (8 MiB per
  process) meanwhile.
- A concurrent request can be answered after a request sent later. Clients that match replies
  by id (the Mac, iOS and desktop clients) are unaffected; a client that assumes replies in
  order must not send these requests without waiting for the reply.
