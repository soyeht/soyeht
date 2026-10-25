# ADR 0079: Never hold the Core lock across blocking IO

- Status: accepted
- Date: 2026-10-03

## Context

The engine keeps all of its state in one `Core` behind one
`Arc<std::sync::Mutex<Core>>`. Everything takes that lock: every request,
and the 10 ms tick that drains terminal output from the supervisor and
pumps it to the clients. The terminal is the owner's top priority, so the
lock is on the terminal path.

Slow work ran while the lock was held:

- Task feedback and `task.request_review` ran git with no timeout
  (`tasks/revision.rs`).
- `task.dispatch`, `task.create` with `dispatch`, and the tick that resumes
  dispatches ran `wt switch`; `task.worktree.clean` ran `wt` and `lsof +D`.
  None of them had a timeout.
- `usage.refresh` read each provider's source one after another, each allowed
  10 s.
- The local server list ran `lsof` twice every 5 s from the tick, and from
  `servers.local.list`.
- `file.diff` ran git for the baseline and blame, with no timeout.
- `file.search` walked and read the whole project.

One slow command froze terminal output and every request on every connection
(Mac, iPhone, iPad, CLI, MCP) for as long as it ran. The catalogue
(`harness.list`, `model.list`) already avoided this with its own worker
(`CatalogJob`, PR #420).

## Decision

**The rule.** No code holds the Core lock while it runs an external command,
a network request, or a walk over a directory tree. Such work has three steps:

1. copy its inputs under the lock;
2. do the work with no lock;
3. apply the result under the lock, checking again any state that may have
   changed meanwhile.

**Request work: `Outcome::Offload`.** A handler returns
`Outcome::Offload(Offload::new(timeout_message, timeout, work, apply))`. The
server runs `work` on a blocking thread with no lock, then runs `apply` under
the lock to produce the reply. `apply` always runs exactly once: with the
work's result, or with an `internal` error carrying `timeout_message` when the
work is late or panics. A handler can therefore release what it reserved in
step 1. For example, usage frees its in-flight providers. A request keeps
its place in its connection's order, while other connections and the tick go
on (ADR 0095 lets a few reads overtake the requests behind them). The protocol is unchanged: a client still gets one reply per request.
`writing.section.list` used to have its own deferred outcome and timeout. It
now uses `Offload`. `task.dispatch` and `task.create` with `dispatch` still
report a failed worktree creation to their caller. They create the worktree
in an `Offload`, then advance the dispatch under the lock.

**Engine-initiated work: `Background`.** Polls, scans and dispatch steps call
`Core::spawn_background(key, work, apply)`. The work runs on a pool of four
threads. The next tick applies the finished jobs under the lock. A key never
runs twice at once. A job whose apply fails waits one second before it can
run again, so a broken `wt` is not retried every 10 ms. Failures are logged
once per change as `engine.subsystem_failed`/`engine.subsystem_recovered`,
with the job family and the error kind, never paths or task names.

**Ports that work borrows off the lock.** The revision, worktree and
port-scan ports are `SharedPort`s (`Arc<Mutex<Box<dyn Port>>>`). Their
own mutex only orders the users of one port and is never taken while the
Core lock is held. The editor's file port is `Arc<dyn FileSystemPort>`, and
`soyeht_vcs::Service` clones share their command port.

**Every external command has a deadline.** The new `soyeht-process` crate
has one runner, `soyeht_process::run(command, Limits)`. It sets a deadline
and a byte budget. A late command is killed with its whole process group (a
job object on Windows, ADR 0086), so descendants do not keep the pipes open.
Both ways of ending "a process and what it started" are in `soyeht-platform`
(`process`), and `soyeht-ptyd` uses the same code for its children. The caller gets
`Error::TimedOut` ("git timed out after 30000 ms and was killed"), which the
engine reports as the subsystem's error, for example `worktree_unavailable`.
git (revision, `soyeht-vcs`), `wt`, `lsof +D` and the local-server `lsof` use
it.

| Command | Deadline |
| --- | --- |
| git for task revisions and `file.diff` | 30 s per command |
| `wt list` | 30 s |
| `wt switch`, `wt remove` | 120 s |
| `lsof +D` | 30 s |
| local-server `lsof` | 5 s per run; after a run times out, the next gets twice as long, up to 15 s, and 5 s again once a run finishes |

**Starts are serialised.** `run` creates a pipe pair for the child, and a
process another thread starts at the same moment can inherit the write ends
before it has replaced its own image. Two mechanisms do this:

- On Unix a fork copies every open descriptor, `CLOEXEC` ones included, and the
  copies are dropped only at `exec`. Until the second program is loaded, the
  new process holds the first command's write ends. On a loaded macOS host that
  load takes seconds, because the system assesses the first run of a new binary.
- macOS has no `pipe2`. `pipe()` followed by `fcntl(FD_CLOEXEC)` leaves a window
  in which a concurrent fork inherits the new descriptors for good, not only
  until its `exec`.

Either way the first command has exited, but its output never reaches the end of
the file, and its collector waits out the pipe grace and reports a timeout for a
command that finished. One start at a time leaves no overlap, so
`soyeht_process::starting` holds one lock per process, in the engine, around
every process start there: `run` itself, the usage JSON-RPC conversation and
the login-shell probe. `soyeht-ptyd` is a separate process with its own pipes,
and it already holds its global state mutex across every request, `Spawn`
included, so it needs no second lock. Hold the engine's lock for the start only;
anything that waits for the new process belongs after it, since every other
start queues behind this one. The time a `run` spends queued counts against its
own timeout, so a stuck start cannot keep a caller past its deadline: the call
returns `Error::TimedOut` when the lock is not free in time. A thread that holds
the Core lock is refused before it can queue, by `run` and by `starting`
alike (`Error::UnderLock`).

What is not done: after a command exits, `run` still waits for the end of its
pipes (up to one second) instead of reading what is already there. Reading
without waiting needs `poll` on Unix and `PeekNamedPipe` on Windows, and a wait
that gives up early could cut off output on a loaded host. The start lock removes
the cause that made that wait long.

**The engine build is never hashed under the lock.** Every experiment capture
records the SHA-256 of the engine's executable, which reads the whole file.
`engine_build::cell()` holds it, started at launch by `prefetch_engine_build`,
and `ready()` only looks. Under the lock `Core::capture_build` is that look and
nothing more. `pane.experiment.set` turning a capture on while the hash is still
running answers with an `Offload` that waits for it and then runs the request
again. Restoring the captures a restart left on does not hold up the first
listen: the wait is a background job, the output of those panes stays in the
supervisor until the job restores them, and so the capture still starts at the
first byte after the restart. That hold is bounded: if the hash is not known
within 60 s the waiting captures stop with a reason and the panes drain again
(the supervisor keeps 8 MiB of output per pane, so a longer wait loses the first
bytes anyway). A `pane.experiment.hook` that comes while captures wait is not
answered as a no-op, which its sender never repeats: it answers with an `Offload`
that waits for the hash, settles the waiting captures and then records. The count of waiting panes is logged once, never their paths. The
result of the hash is kept, a failure included: it is logged once, by kind, and
a later enable answers with it at once. `EngineBuildCell::wait`, like `run`,
refuses a thread that holds the Core lock.

**The lock is taken in one place.** `core_lock::lock` and `core_lock::with`
are the only ways to take it. `with` runs on a blocking thread, so no async
worker ever waits for the lock. The guard marks its thread with
`soyeht_process::forbid`, and `soyeht_process::run` refuses to start a
command on a marked thread (`Error::UnderLock`). The guard also logs any hold
of 100 ms or more as `engine.core_lock_slow`, with a short static purpose
such as `tick`, `dispatch` or `offload_apply`. A hold that repeats is one condition: per
purpose the guard logs the first slow hold, a hold at least twice as long as the longest one
logged since, and once a minute `engine.core_lock_slow_summary` with the `count` of the holds
left out and their `worstMs` (`slow_holds.rs`, the rule of `Streak`, AGENTS.md "Logging"). The
line is written after the lock is released.

**One failing subsystem cannot stop the tick.** `poll_output` runs each
subsystem (editor, project tree, local servers, web commands, harness
switches, retired agents, experiment captures, the task steps) in isolation,
logs its failure under its name, and always drains the panes.
`TICK_INTERVAL` (10 ms) and `PRUNE_EVERY_TICKS` (6000, once a minute) are
named constants.

**Enforcement.**

- At runtime, `soyeht_process::run` refuses to run a command under the lock.
- `tests/blocking_io_rule.rs` scans every crate that runs inside the engine.
  It fails when shipped code runs a process without `soyeht_process::run`.
  Three files are allowlisted, each with its reason:
  - the login-shell probe (`agents/environment/probe.rs`), which runs in
    `Core::open` before the Core is shared;
  - the usage host's process runner (`usage/process.rs`) and hidden terminal
    (`usage/terminal.rs`). `curl` and `security` go through
    `soyeht_process::run_with_input` (the credentials reach `curl` on standard
    input). What remains outside the runner is the Codex JSON-RPC child, which
    is a conversation with one long-lived process, and the pseudoterminal.
    They run only from `PreparedRefresh::read`, off the lock, each with its own
    deadline.

  The usage refresh itself follows the rule. `usage.refresh` is an `Offload`
  whose apply releases its providers when the read is late, and the usage
  schedule takes the lock through `core_lock` only to choose and to commit
  (ADR 0077 owns the usage sources).
- `tests/core_lock.rs` holds a fake usage source blocked for 5 s while a
  shell prints a line every 20 ms. It measures the longest gap between
  terminal frames that reach a second client. It also measures the latency of
  `engine.status` on a third connection.

  The allowed gap is four times the gap measured on the same rig with no
  slow work. It is at least 750 ms and at most 2.5 s, half the blocked time,
  so the old behaviour cannot pass.

  On an idle machine the gap was 147 ms under the slow source against 156 ms
  without it, and `engine.status` answered in 1 ms. With the work run under
  the lock again, the gap was 5.26 s and `engine.status` took 4.7 s, so the
  test failed as it should.

## What still runs under the lock

The rule covers external commands, network IO and tree walks. These remain
under the lock, and each is bounded:

- The terminal path itself: draining a pane goes through the supervisor
  socket (5 s timeouts) and appends to that pane's screen, which Core owns.
  Taking that path off the lock entirely needs per-pane state or a Core actor.
  That is the long-term item H3 of the 2026-10-03 quality audit.
- SQLite reads and writes, and the minute prune.
- Single-file reads and writes of bounded size: editor open, the save of a pending
  draft before a delete (the autosave, the disk watch and the save of a closed tab run off the
  lock, ADR 0097), one project folder listing, one skill folder listing after a file event
  (ADR 0075), writing documents, settings and registry files.
- `file.search` with a replacement still plans and writes under the lock, so
  the editor buffers, the disk and the replacement journal stay consistent.
  A plain search runs off the lock.
- `pane.experiment.export` writes its export under the lock. It is a rare
  action, available only in the signed app.

A slow hold among these appears in the log as `engine.core_lock_slow`.

## Consequences

- A hung git, `wt`, `lsof` or usage source delays only its own reply or
  background job. It never delays terminal output or other requests.
- Handlers that offload validate twice, before the work and again when they
  apply it. A change made meanwhile (a moved task, a reserved dispatch that
  advanced) makes the apply a no-op or an error, never a stale write.
- While a usage refresh is reading a provider, a second refresh of that
  provider is refused as `rate_limited`, as it already was within the 30 s
  refresh interval.
- A new blocking operation must use `Offload` or `spawn_background`, and run
  its commands through `soyeht_process::run`. The tests above fail if it does
  not.
