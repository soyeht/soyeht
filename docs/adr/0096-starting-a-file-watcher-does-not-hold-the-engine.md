# ADR 0096: Starting a file watcher does not hold the engine

- Status: accepted
- Date: 2026-10-19

## Context

The engine log of a daily-use session showed `project.tree.watch` taking 1,041 ms with
`engine.core_lock_slow purpose=dispatch heldMs=1041`. The request started the recursive file
watcher of the workspace (FSEvents on macOS, through the `notify` crate) and read the first
listing of every folder it was asked to follow, all with the Core lock held. Asking the
system's file-event service to start a stream takes tens of milliseconds on an idle machine
and a second or more on a busy one. For that long no request on any connection and no
terminal output of any pane was served (ADR 0079).

The skill folders already start their stream on a thread of their own
(`skills/watch.rs`), so they do not wait for it.

## Decision

`project.tree.watch` is an offloaded request (`Offload`). Under the lock it checks the
parameters, reads the workspace path, and asks whether the workspace already has an event
source. Off the lock it opens the project root, checks every folder, starts the event source
when the workspace had none, and only then fingerprints the first listing of each folder, so a
change after a fingerprint is always an event. Under the lock again it keeps the source and the
fingerprints.

Two requests for the same workspace can both find no source and both start one, and a source
can be dropped as unwatched while a request is off the lock. In both cases a change between a
request's listing and the source that stays is in no event, so the workspace falls back to the
source that stays (polling when none stays) and every folder is read once more; folders whose
listing changed are announced.

The same gap exists when the workspace already has a live source: a tick that takes the events
of the workspace between the request's listing and its apply clears them before the request's
folders are stored, so nobody is told. `Watches` counts an epoch that the tick bumps whenever
it takes events out and that dropping a source bumps. The request takes the epoch under the
lock with its plan, and the apply reads every folder once more when the epoch moved. The apply
also finds the workspace again (a workspace closed meanwhile is not found and nothing is kept),
and a source that is not kept is dropped on a thread of its own, not under the lock.

The request keeps its place in its connection's order: a client that sends two watches for one
workspace gets them applied in order. Other connections and the tick go on.

## Consequences

- A slow start of the file-event service delays only the reply to the connection that asked.
  `tests/project_watch_start.rs` fails on the old code: the test engine pauses before each
  stream starts (`SOYEHT_TEST_SLOW_WATCH_MS`, compiled only with the `test-hooks` feature).
- `project.tree.watch` with no folders still only drops the connection's watch, under the lock.
