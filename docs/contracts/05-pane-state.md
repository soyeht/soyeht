# Contract 05 · Pane state

Status: PARTIAL, decided by Caio on 2026-09-10. The agent pane (ACP) is
settled. The raw terminal pane awaits a data-driven EXPERIMENT;
this contract defines what the engine must prepare for that experiment.
Source: (internal analysis, outside the repository) (internal analysis).

## 1. States clients receive (both pane types)
`working`, `idle`, “waiting for you” (permission or question), “no
signal”, `dead`, and **“unknown”** (only in the raw terminal pane when
confidence is low; no other app has this, and it prevents guessing).
The card phrase (contract 03 §6) is calculated from this.

## 2. Agent pane (ACP) — SETTLED
Comes from the protocol (contract 04 §8): accepted prompt → `working`;
open `session/request_permission` → “waiting for you”; `idle` → `idle`;
no notification for 90 s with an open turn → “no signal”; process exited →
`dead`. Exact; no screen reading.

## 3. Raw terminal pane — DECISION DEFERRED TO AN EXPERIMENT
Caio's decision: do not choose between a hook, marker or regex now. Decide
using data, like a data scientist, after measuring several harnesses.
His hypothesis: a lightweight classifier over the screen should work better
than handwritten rules (“like email: rules did not work, a classifier did”).
Simple starting signal: changing screen = working; static screen = stopped.

Rules already in effect, drawn from others' mistakes (issues):
- never declare `idle` before the first sign of life; `task_next` and the
  loop send prompts only after the first real `idle`;
- “waiting for you” only with a real request; any new signal clears it;
- partial coverage appears as “unknown”, never as `idle`;
- if hooks are introduced, they follow the harness contract (JSON stdout,
  short duration) and are not written into the user's shared configuration.

## 4. What the engine must prepare for the experiment (included in v1)
1. **Pane frame recording**: on each screen change, store
   `{pane, harness, harness version, t, OSC title, last N lines,
   cursor position, foreground process, bytes per second}`.
   Format: rotated JSONL per pane; enabled per pane (“record for experiment”).
2. **Ground-truth label**: for harnesses with official hooks (Claude Code,
   Codex), record timestamped hook events in the same stream as labels
   (“started”, “called tool”, “requested permission”, “stopped”).
   For ACP panes, the protocol itself provides the label.
3. **Export dataset**: an engine command combines frames + labels from a
   period into a file for analysis outside the engine (Python).
4. **Detector interface**: the engine calls a per-pane function
   `state(frame) -> (state, confidence)`; its implementation may be regex
   (baseline: reference screen rules and those in the current Swift app), the
   “changing screen” signal, or a classifier; replaceable without changing the rest.
5. **Metrics**: precision and recall by state and harness, emphasizing
   “waiting for you”; detection latency; “unknown” rate.

## 5. Experiment plan (later, with Caio)
Record hours from several harnesses; compare reference screen rules, current
app rules, the “changing screen” signal and a lightweight classifier against
the labels; decide per harness. Learn from issues: “notification with two meanings”,
“sticky state”, “background shell”, “permission prompt that looks idle”.

## Review applied (2026-09-10, finding R12)
One enum for both pane types: `starting`, `working`, `idle`, `waiting_for_you`, `cancelling`, `no_signal`, `recovering` (engine reconnecting to ptyd), `dead`, `unknown`. Each state carries `source` (`protocol | hook | osc | screen | none`) and `confidence` (0–1). `waiting_for_you` requires an open request; `no_signal` neither clears a request nor releases the loop/`task_next`; `starting` never becomes `idle` without a real signal; for an agent pane that signal is the open session (`pane.session_started`).

## Exit status of a dead terminal
A terminal pane in the `dead` state carries `exitCode` when the supervisor recorded the process's
exit status (a pane adopted without one has none). Clients decide from it how the exit reads: a
status of 0 only informs ("Terminal process finished."), so a one-shot command such as an agent
sign-in ends clean; a positive status warns ("Terminal process exited with status N."); a process
that ended without a status (a negative value) warns too. `terminalNotices` of
`contracts/vectors/pane-state-presentation.json` fixes the cases that the Mac and desktop clients pass.

## The harness a terminal pane runs (ADR 0165)

A terminal pane has a harness when the foreground program of its terminal is one the engine knows: Claude Code,
Codex, OpenCode or Cursor Agent (not any other program, and not a harness in WSL, `tmux`, `ssh` or a container, whose
processes are not the pane's). The pane then carries `terminalHarness` with `id` (the registry id the apps know it by),
`name` and, when its install path carries one, `version`. It is absent while the foreground is a shell or anything else,
and `pane.updated` carries every change of it.

**What the supervisor reports.** `Foreground` (the `foreground` feature of the supervisor's welcome; an engine sends it only to a supervisor that lists it, and a supervisor that does not leaves every terminal without a harness) lists, for terminals the engine names, the programs
in their foreground, each with its pid, executable path and `argv`. On macOS and Linux it is the leader of the terminal's
foreground process group (`tcgetpgrp`); on Windows, which has no foreground group in a pseudo console, it is the
processes of the pane's job in tree order (a parent before its children, a parent id counting only when that process is
no younger than its child, since ids are reused), and the engine takes the shallowest known harness reached from the
shell only through shells and launchers, never one of a harness's own children.

**How the engine tells a harness.** By a table of data (`terminal_foreground/harnesses.rs`) over the executable path,
`argv[0]` and the script argument of a host program such as `node`. A program is a harness when, and only when:
(1) a file it runs (the executable, or the script of a host program) sits in an install layout of that harness
(`.local/share/claude/versions/`, `.local/share/cursor-agent/versions/`, `Caskroom/codex/`, `Cellar/opencode-v2/`,
`node_modules/@openai/codex/`, `node_modules/opencode-ai/`, and so on, as the spike measured them); or (2) `argv[0]`
is the bare command name, so the shell found it on the person's `PATH`; or (3) a file named like the command (the executable, `argv[0]`, or the script of a host program) sits in a
folder of the person's `PATH`: a harness installed with npm runs as `node` and the link npm made in a `PATH` folder, as
it was invoked (measured on macOS and Linux). A binary or script merely named like a harness in a project folder is none of these and
is not matched. The rule is a heuristic, and a wrong match only mislabels an entry of `agent.list`: it grants nothing,
and slice 3's check of the terminal's screen decides whether anything may be typed. For the same reason a run that is
not the interactive program (`-p`, `exec`, `--help`) is listed as the harness while it runs; the screen check decides
whether it can receive. Sign-in terminals of an agent pane are never read or listed. The matcher is tested on the shapes
as they were measured, and on look-alikes.

**What is reported.** A program is reported with its executable path (at most 1000 bytes), at most 8 arguments of at
most 400 bytes (`argv[0]` and the script argument are what the matcher needs), and at most 64 programs of a job; a job
read on Windows is read for at most 256 process ids. The whole answer is held to an eighth of the largest JSON frame:
a terminal whose programs do not all fit is marked `truncated`, and terminals past the budget are left out of the
answer. The engine keeps what it knew of a pane that is left out, and of one that is `truncated` and shows no harness
(none shown does not say none runs); a `truncated` answer that does show a harness is applied.

**The version.** Read from the install path, from the part (or the one after it) that follows the harness's own install
layout (`2.1.325`, `0.169.0`, `2026.09.02-c22c1a3`): three numbers joined by dots, with an optional suffix. A version
elsewhere in the path (a node manager's folder) is not read, nor one in a path the supervisor cut short. No program is ever run to ask. A path that carries none gives
no version.

**When it is read.** Never on a period for every terminal. A pane is read on demand (`agent.list` reads every terminal
unless the last read is younger than a second) and otherwise only when it has printed since its last read (or has just
been adopted after an engine restart, so a harness already running and idle is labelled), a change of
the foreground program always printing, those reads coalesced to one per 2 seconds and only while an app subscribed to
events watches. An idle shell costs nothing. A pane restored after an engine restart keeps the harness it had until it
prints; one whose process ended loses it at once. `agent.list` never fails for the sake of a terminal: with no supervisor, a
supervisor that does not list the `foreground` feature, or a read that fails, it answers with what is known. When a
read is due it waits for the supervisor's answer, which is bounded by the supervisor link's own timeout.
