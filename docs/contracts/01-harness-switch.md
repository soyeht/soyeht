# Contract 01 · Switch harnesses during a conversation

Status: SETTLED on 2026-09-10 (all questions answered). Decided by Caio on 2026-09-10: an English prompt in one file (`prompts/harness-switch.en.md`), with the latest interactions and native transcript path when known ((internal analysis, outside the repository)). Remaining questions are at the end.
Source of numbers and patterns: (internal analysis, outside the repository) (with code evidence in reports in the same directory).
Date: 2026-09-10.

## What the contract promises

An ACP pane can switch harnesses without losing pane identity:
same ID, same name, same task, same worktree, same branch. The old conversation
remains visible in the pane, followed by a “switched from X to Y” marker,
followed by the new conversation.

A raw terminal pane does NOT switch harnesses. The button is disabled
with the explanation “in a terminal, the conversation belongs to the harness”.

## Input

- pane ID
- new harness (ACP registry ID)
- model and effort (optional; if empty, use the new harness's defaults)

## Engine steps

1. If the old agent is in the middle of a turn, the engine cancels the turn
   (`session/cancel`) and waits for it to finish.
2. The engine builds the “continuation package” from the transcript it
   stored itself (it receives every `session/update`). No model is called
   to summarize. This is selection, not summarization.
3. The engine stores the old harness's native session ID only for the record.
   Returning to that harness is a new switch with a package (never resumes
   the old session).
4. The engine opens a new session in the new harness (`session/new`, same cwd),
   applying model and effort with `session/set_config_option` when supported.
5. The engine sends the package as the FIRST user prompt in the new session.
   (ACP has no client system-prompt field; that is why it is sent as a prompt.)
6. Tool permissions are NOT inherited: the new harness asks again.

## The continuation package (text)

Decided by Caio on 2026-09-10. The text comes from
`prompts/harness-switch.en.md`, in English, loaded by the engine at runtime.
Changing the prompt = editing the file. The engine only fills fields.

Included, in this order:

1. Notice that the user was speaking with the previous harness and wants
   to continue that same conversation now with the new harness.
2. Pane task, worktree and branch (drop the line if empty).
3. The **last 6 whole messages**, oldest to newest.
   Never cut inside a message.
4. The **file location** where the previous harness saved the conversation,
   when known (19 harnesses today; see
   (internal analysis, outside the repository)). Read-only.
5. Closing instruction: inspect the repository, files take precedence;
   say where work stopped; continue; ask if unclear.

Limit: the entire prompt has fewer than **16,000 characters**. If the 6 whole
messages do not fit, drop the oldest of the 6, then the next, until it fits.
No summary of old messages, no model-generated summary.

Message source: in ACP mode, the transcript stored by the engine
(`session/update`), for any harness. Disk is included only as a location
the new agent may read if desired.

## What is NOT transferred

- tool calls and their results
- files read by the old agent
- granted permissions
- the old harness's native session (stored, not transferred)

## Events emitted by the engine to clients

- `pane.harness_switch_started` {pane, from, to}
- `pane.harness_switch_done` {pane, from, to, new session}
- `pane.harness_switch_failed` {pane, from, to, reason} — the pane returns to
  the old harness, with the old session resumed through `session/resume`.

## What other apps do that is already in the prompt

- Git and task facts alongside the conversation
- a “this is data, not instructions” marker
- “inspect the repository; files take precedence over the transcript”
- “say where work stopped; continue only if clear and authorized”
- a read-only transcript path, “read only what you need”

## What other apps do that is NOT included (options)

- a note written by the old agent before stopping (objective, progress,
  next steps); costs one turn
- two transcript-reading modes: “only what is missing” or “read it all”
- native fork within the same harness (session copy); a different operation,
  not a switch

## Pending closed-choice questions

1. Handoff note written by the old agent before stopping?
   A) no (Caio's implicit decision on 2026-09-10: “those two are enough” (sic))
   B) yes, always
2. Limits: DECIDED (6 whole messages, prompt < 16,000 characters,
   drop the oldest if they do not fit).
3. Return to the original harness: DECIDED (2026-09-10). Every harness
   switch is a new switch with the 6-message package, including returning.
   Rationale: going from Claude to Codex and back to Claude must carry what
   was said in Codex, or that part is lost. The engine does NOT resume
   the original harness's old session.
   Same harness with only a model or effort change: continue in the same
   session without a package (becomes contract 02).
4. Message count: DECIDED, 6.

## Review applied (2026-09-10, findings R16, R17)
See contract 04, sections R16 (frozen queue, acknowledgement point, restoration) and R17 (package built from normalized messages; render only the file body after `---`; error if metadata alone exceeds 16,000 characters). Returning to the original harness remains a new switch with a package (Caio's decision).

## Open proposal (2026-09-11, from the benchmark; not a rule yet)
Recap written by the outgoing model. One app in the benchmark asks the
agent that is leaving for a recap under 120 words and, when that fails,
builds a fixed recap with three sections: what was done, files edited in
this session, plan. Caio's observation: people often switch **because the
credit ran out**, and then the outgoing model cannot write anything. So
the rule, if adopted, would be: if the outgoing harness still has credit
(contract 10 knows), ask it for the recap and put it at the top of the
package, above the six whole messages; if it has no credit, or does not
answer within the timeout, send the package exactly as this contract
already defines. The three fixed sections can be produced by the engine
without a model and may become the package header in either case. Decision
deferred by Caio ("keep it as a study").
