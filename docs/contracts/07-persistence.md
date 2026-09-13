# Contract 07 · Persistence and surviving restarts

Status: SETTLED on 2026-09-10 (Caio's decisions at the end). Evidence of today's
behavior: memory `reference_soyeht_persistence_today_2026_09_09`; comparison
with internal analysis. Rule: preserve today's model
(launchd + engine + ptyd + segments) and fix its weak points.

## 1. Three processes, three lifetimes
| Process | Supervisor | Dies when | Holds |
|---|---|---|---|
| App (Mac/Linux/Windows/iPhone) | user | window closes | nothing durable |
| Engine | launchd, KeepAlive | update, crash | state (database), ACP as CLIENT, board, loop, lead agent |
| `ptyd` (process supervisor) | launchd, KeepAlive, separate | only machine restart or explicit `stop` | ALL pane processes: shells, harnesses in raw terminals, ACP harnesses and language servers |

**Decision A (Caio):** `ptyd` also holds agent harnesses (ACP).
For PTY it provides a pseudoterminal; for ACP and LSP it provides stdin/stdout pipes.
The engine speaks to the ACP harness through `ptyd` (local socket), never
as a direct child process. Updating the engine = engine restarts, reconnects
to `ptyd`, and the harness continues its turn without noticing.
Updating `ptyd` = rare; requires a “will stop agents” warning and all panes
idle, or descriptor handoff (outside v1).

## 2. One owner, one database
- **Owner of all durable state: the engine.** Apps never write durable files
  (end of Swift writing `workspaces.json`).
- **One SQLite database** (`soyeht.db`), with `user_version` and numbered
  migrations, WAL enabled, engine-only writes. Tables: workspaces,
  panes, ACP sessions (native ID, cwd, harness, version), ACP transcript
  (messages), tasks, subitems, comments, attempts, loops, paired devices,
  events (append-only log with increasing IDs for reconnection and iPhone).
- CBOR only where an append-only signed log already exists (`owner_events`);
  everything else moves to the database. `ptyd` stores its process registry
  in its own file (currently RAM-only: fix it) for reconnection after engine restart.
- Screen (PTY segments): as today, 512 KiB per segment, 32 MiB per pane,
  replay by offset; **one** format (retire the legacy flat log). Recording
  enabled by default, capped, with a “do not record this pane's screen” button (secrets).

## 3. What returns after each event
| Event | Raw terminal | Agent pane (ACP) | Board/tasks | iPhone |
|---|---|---|---|---|
| close app | everything continues (engine + ptyd alive) | same | same | same |
| engine update/crash | process lives in ptyd; screen replay; nothing lost | harness lives in ptyd; engine reconnects and resumes the ACP stream in place; nothing lost | database | reconnects and requests full state |
| ptyd restart (rare) | process dies; a new shell returns in cwd + recorded screen; agent relaunched by native ID (per-harness resume command table in internal analysis) | harness dies; `session/resume`; if unavailable, contract 01 | database | same |
| Mac restart | same as above, for everything | same | database | same |

Rules drawn from others' issues: never delete a native session ID because of
time; do not resume a pane deliberately closed by the user; never leave an
old `ptyd` generation running invisibly (only one, adopted by the new engine,
with a protocol version on the socket).

## 4. Caio's decisions (2026-09-10)
1. ACP harness under `ptyd`: **A**.
2. Database: **one SQLite database**, “if that is the best idea” — it is:
   8 of the 10 apps use it, with migrations.
3. Recorded screen: enabled by default, capped, with an off button
   (assumed, not contested).

## Review applied (2026-09-10, findings R04, R09, R24)

**Engine ↔ ptyd channel (R04).** Its own local socket, with a protocol version. For each process, `ptyd` stores: `instanceId`, `instanceKey` (spawn idempotency key supplied by the engine), `generation`, type (pty | pipes-ACP | pipes-LSP), cwd, argv, filtered env, and an **8 MiB retention buffer per process** (terminal bytes, ACP messages or LSP frames) with sequence numbers. Retention owner: `ptyd`, only until `ack`. Owner of durable segments (recorded screen, 512 KiB × 32 MiB per pane (sic)): the engine, which writes them before sending `ack {seq}`. If retention fills without `ack` (engine absent too long), `ptyd` does NOT block the child process: it discards the oldest bytes and records a `gap {fromSeq, toSeq}` marker; on reconnecting, the engine turns that marker into a segment gap record and the client sees “missing section” (contract 08 §6). Declared loss, never silent. Spawn is idempotent by `instanceKey` (a second request with the same key returns the same process); `close {instanceId}` is idempotent (closing an already closed process returns ok and never relaunches it). The engine records “spawn requested {instanceKey}” BEFORE calling `ptyd` and reconciles on reconnection: a process with that key exists → adopt; no process → repeat the request. On reconnecting, the engine presents `engineGeneration` (increasing, persisted in the database); `ptyd` accepts ONE engine at a time and **refuses a generation lower than the last accepted** (`stale_engine_generation`), so an old engine cannot evict the current one. `ptyd` replays everything since the last `ack` and the list of unanswered ACP harness requests; LSP request correlation belongs to the engine. `ptyd` registry in a file (not only RAM). Incompatible version: the engine reports `ptyd_unavailable` and does NOT open another `ptyd`; never two live `ptyd` generations.

**Reconcile before returning tasks (R09).** After restarting, the engine first asks `ptyd` which processes are alive; it returns tasks only from panes whose process no longer exists. Partial dispatch (task reserved, worktree created, pane not opened) is resumed or undone using the recorded steps.

**Durability (R24).** Data is acknowledged to the client only after a SQLite WAL commit with **`synchronous=FULL`** (synchronizes on commit; `NORMAL` may lose acknowledged commits on power loss), with periodic checkpoints. Declared limit: the guarantee is that of Mac storage (SSD with honest caching). Full disk: durable mutations are refused with `disk_full` and an attention-inbox notice; live processes are not killed. Separate configurable retention for: screen (segments), ACP transcript, tasks/history (unlimited), experiment dataset (contract 05). “Do not record this pane's screen” disables both segments AND that pane's dataset. Migration: automatic database backup before migrating; an engine older than the database refuses startup with a clear message (no silent downgrade).
