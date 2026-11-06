# Slice 1 · the smallest end-to-end proof

Status: scope defined and cleared by review (aria, 2026-09-10).
Awaiting Caio's “you may start”. No code yet.

## Included scope
- **Dev** profile (data in its own directory, its own port; current Soyeht is untouched).
- Engine (Rust) and `ptyd` (Rust) as two launchd services (contract 07).
- Protocol schema (`contracts/protocol.json`) with this slice's methods and
  events and generated Rust, Swift and TypeScript types (contract 08 §1.7).
- CLI: `soyeht pane create --shell` creates ONE raw terminal pane.
- New Mac app (Swift): connects through the local socket, displays the pane
  and screen, and accepts keyboard input.
- New iPhone app (Swift): pairs by QR over the tailnet (08 §2.2), connects
  and displays the same pane (read-only).
- Single versioned SQLite database, `synchronous=FULL` (07).
- Screen segments with offset-based replay, `tail`, delta and `gap` (08 §6).

## Acceptance (all must pass, in CI and manually)
1. Close and reopen the Mac app: the pane remains and the screen returns without a gap.
2. `kill -9` the engine and restart through launchd: the shell process is the
   SAME (same PID), the engine adopts it through `ptyd`, the screen continues,
   and the iPhone reconnects automatically and displays the same content.
3. Disconnect and reconnect the iPhone (airplane mode for 1 min): `hello` →
   `snapshot` → `subscribe`, without lost or repeated events.
4. Close the pane through the Mac: it disappears on the iPhone; restarting the
   engine does not resurrect it.
5. Repeat `pane.create` with the same `idempotencyKey`: just one pane.
6. Fill `ptyd` retention while the engine is stopped: on return, “missing section”
   appears, never false continuity.
7. Restart the Mac (separate test): the old shell dies; layout, cwd and recorded
   screen return; the pane starts as a new shell, explicitly marked.

## Excluded scope
Agent pane (ACP), task list, loop, lead agent, harness switching,
Linux/Windows app, push, hook/regex-based state.

## Slice 2 (later, direction already decided)
An ACP v1 harness in an agent pane: prompt, permission, cancellation,
recovery after restarting the engine (harness alive in `ptyd`).
