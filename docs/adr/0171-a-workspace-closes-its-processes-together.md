# ADR 0171: A workspace closes its processes together

- Status: accepted
- Date: 2026-11-20
- Builds on ADR 0117 (the supervisor link off the Core lock), ADR 0119 (agent polls and closes
  off the Core lock) and ADR 0165 (features of the supervisor's welcome).

## Context

Closing a workspace of 60 terminals took seconds, about 40 ms for each terminal on a quiet
machine, and the supervisor stood still for all of it. Measured in the supervisor, each `Close`
did three things one after another, all under the state lock that every request and every
terminal's output shares:

1. it wrote the whole registry durably (a file sync and a directory sync), 12 to 23 ms;
2. it killed the process group and polled for the exit every 5 ms, 6 to 8 ms;
3. when the exit watcher of that terminal saw the exit, it wrote the whole registry durably
   again, under the same lock, for a status of a process that was already closed.

The engine sent the closes of a workspace one request at a time over its one connection to the
supervisor, so the costs added up and nothing could overlap.

## Decision

1. **`CloseMany`.** One request names up to `MAX_CLOSE_MANY` (256) processes. The supervisor marks
   them closed and writes its registry once, under the state lock, then releases the lock and ends
   all of them together (`Child::end_together`). The limit of the wait (2 s) starts before the first
   process is ended. On Unix the process table is read once for every agent's tree
   (`soyeht_platform::process::kill_trees`; reading it for each tree cost seconds for a few hundred
   of them), and on Windows every job is terminated first and all are then waited for. The wait is
   that of the slowest process and not the sum of them, and the pause between two looks starts at
   half a millisecond. The answer comes when every process is gone, so the guarantees of `Close`
   hold for each: a process that ignores the polite signals is killed (the signal is `SIGKILL`
   for a group, and the whole tree for an adapter), and a process the supervisor does not know or
   has closed already is not an error. Two limits of "gone". On Unix only the child itself is
   collected (no zombie of the supervisor's own); the rest of its tree is signalled, and a
   process that left the group can stay a zombie under its new parent for a moment after the
   answer. Windows waits for the whole job. And a process that outlasts the 2 s limit is logged
   once for the batch (`ptyd.child_still_running_after_kill`, with the count) and left to its
   exit watcher, which collects it when it does end: the answer is still `ok`, as it was for
   `Close`. A poisoned lock of a process is recovered and the process ended. A process that cannot
   be looked at (`try_wait` fails, which on Unix means it is not the supervisor's child any more
   and its pid may belong to another process) is never signalled; the failure is reported after
   the others are ended. A request over the limit is refused as `invalid_params`.
2. **`Close` is `CloseMany` of one**, so a single close also waits without the state lock, and
   the wait polls finer than before.
3. **An exit watcher writes the registry only for a process that is not closed.** A closed
   process is not listed, and a restart of the supervisor marks every process closed when it loads the registry,
   so its exit status is not worth a durable write of the whole registry under the state lock.
4. **A second close waits for the first.** The supervisor keeps the processes a close is ending in
   a `closing` set until nothing holds them. A close of a process that is already closed but still
   being ended (a new engine's, while the connection it replaced is still closing) waits for it
   through the lock of the process, and does not answer before it is gone.
5. **A feature, not a version.** The supervisor lists `close-many` in its welcome
   (`FEATURE_CLOSE_MANY`), as it lists `foreground` (ADR 0165), so a running supervisor of the
   protocol version that does not list it keeps working after an engine update. The engine's
   `workspace.close` ends the processes of the workspace's panes in one job when the link lists
   the feature, in batches of at most 256, and otherwise, or when a batch fails, closes each
   process with its own `Close`, which settles and retries each by itself (ADR 0119). The wire
   of the engine's protocol (`contracts/protocol.json`) does not change.

## Consequences

- 60 terminals close in about 50 ms on a loaded development machine (2.3 s before), and a
  `workspace.close` answers in a few milliseconds, as before: the processes end off the Core lock.
- The registry holds closed instances for as long as the supervisor lives, and each write is of
  all of them. Pruning them is a separate change: a spawn with the key of a closed instance
  answers with that instance today.
- A close of one pane writes the registry once, not twice.
- Follow-up: the durable write of `closed` has no reader (a restart marks every instance closed
  when it loads the registry), so dropping it would make every single `pane.close` fast too. It is
  left as it is here.
