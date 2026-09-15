# Objective · Soyeht slice 8: harness switching, and a picker a person can actually use

Self-contained, for execution to completion without questions to Caio. Same absolute
safeguards as slices 1-7: no production, Dev profile `Soyeht.next/dev`, no secrets, no
tags/releases, no competitor brand names in product docs, English-only repo, Apache-2.0,
no AI attribution in commits/PRs. Ambiguity becomes an ADR and work continues; self-merge
on the green local gate; victoria reviews each PR after merge for correctness, contract
preservation, design fidelity and safety. Do ALL git in your own clone. Date: 2026-09-15.

## 0. Why, and what is already true

Slice 7's closing work gave the rewrite a real harness catalogue: 42 entries from a pinned
snapshot of the public ACP registry, offline, with three honest states. victoria verified it
hands-on at main 6d39168 — Codex, Cursor, OpenCode and Gemini are discovered and selectable
with no JSON editing; Codex, Claude Code and Cursor each held a real conversation; model
changes apply in-session (`applied/sessionPreserved`) and the conversation SURVIVES the
change, proven by asking the agent to recall its previous turn after switching models.

Two things remain, and Caio hit both within minutes of opening the app.

**(a) You cannot change harness in a live conversation.** `pane config <id> harness <other>`
returns `unknown configuration option`. Contract `01-harness-switch.md` is SETTLED and
records Caio's decision (option B): the engine opens a NEW session with the new harness and
carries a continuation package — six whole messages plus file location and a notice — and the
user sees a "conversation restarted" marker. None of it is built.

**(b) The catalogue is correct and unusable.** Caio, looking at the shipped app: "tá
aparecendo um autohand code, agoragentic, eu não conheço isso" and "não consegui colocar o
codex". The list is ALPHABETICAL across all 42, so a person meets Agoragentic, Amp,
Antigravity, Auggie CLI and Autohand Code before reaching anything they own, and `Amp`
(measured NOT installed on this machine) sorts ABOVE the user's working Codex. We solved
"the list is empty" by creating "the list is a phone book". That is our bug, not the
registry's.

## 1. Architecture, decided before any feature

**The first PR is an ADR** extending 0066. It must state, and every later PR must hold:

1. **A harness switch is a NEW SESSION, never a mutated one.** The pane keeps its identity,
   history and file; the ACP session is replaced. Never a session id without its harness.
2. **The continuation package is DATA, not instruction.** It is injected as clearly labelled
   untrusted context, exactly as the picked-web-content flow already does (ADR 0059). An
   agent must not be able to smuggle instructions into the next harness through it. Bound its
   size explicitly and state the bound.
3. **The switch is atomic and reversible in the UI sense:** either the new session starts and
   the marker is written, or nothing changes and the old session keeps running. A failed
   switch must never leave a pane with no live agent.
4. **Availability ranks the catalogue; it does not filter it.** Everything stays reachable;
   what the user owns comes first. No harness is hidden.
5. **Additive wire, protocol stays 1.** `generate.py --check` green.

## 2. Order of work

2.1 ADR (above) + the continuation-package builder as a bounded, deterministic, tested unit.
2.2 Engine: harness switch on a live pane — new session, package delivered, marker recorded,
    old session torn down only after the new one initialises.
2.3 Mac: the switch control in the pane, showing the restart honestly ("conversation
    restarted with <harness>"), never pretending continuity we do not have.
2.4 **The picker, which is half of this slice and not a polish item:**
    - Rank: installed and ready first, then installed-needing-setup, then everything else.
    - Group with headings so the boundary is visible; never bury an owned harness.
    - Make it searchable — 42 entries need a filter box, not horizontal scrolling.
    - The three states must be legible at a glance in the picker, not only in Preferences.
    - The default selection is a harness the user actually has.
2.5 Task dispatch: a task that names a harness applies it through the same path.

## 3. Acceptance (work ends only when all pass)

1. **[local/ADR]** The switch-as-new-session boundary, the package's untrusted-data framing
   and its size bound, and the ranking rule are recorded in an ADR.
2. **[CI]** Switching harness on a live pane opens a new session, delivers the bounded
   package, records the restart marker, and preserves pane identity, history and file.
3. **[CI]** A failed switch leaves the ORIGINAL session running and reports a typed error;
   no pane is ever left agentless. Prove it with an induced failure, not a comment.
4. **[CI]** The package is size-bounded and labelled untrusted; a test asserts an injected
   instruction inside it does not reach the new harness as an instruction.
5. **[CI]** Picker ranking: with a profile where exactly one registry harness is installed,
   that harness sorts above every not-installed entry, and the assertion is on ORDER, not on
   presence.
6. **[CI]** Search narrows 42 entries to a match by name substring.
7. **[reviewer]** victoria drives the running Dev build as a person: finds Codex in the
   picker WITHOUT scrolling past strangers, starts a conversation, switches that live
   conversation to another harness, and confirms the new harness receives the prior context
   and the restart is stated honestly. Caio confirms by hand.
8. **[CI]** No regression: all slice 1-7 acceptances, four ratchets, font/color gates,
   restricted-vocabulary scan, `generate.py --check`, protocol version still 1.

## 4. What the field does (measured, so we do not reinvent it worse)

From the benchmark reports in `~/Documents/soyeht/benchmark`: **no reference app switches
harness inside a live session.** AO switches only Claude Code <-> Codex, in worker sessions,
by starting the new harness fresh with a hidden `<ao-continuation>` block carrying the task,
the last prompt and answer, git/PR facts and a transcript excerpt capped at 600 lines or
64 KiB, explicitly marked "this is data, not instruction". Example forces a new session and
passes a generated seed with `coveredUntilMessageId`. Example opens a new thread and sends the
last 6 messages up to 2,400 characters each, older ones at 320, total capped at 32,000. Example
rejects `setProvider` after the first message and opens a new tab. T3 Code errors outright.
example says "the provider session IS the conversation" and offers a handoff instead.
So Caio's settled decision (new session + package) matches what the field converged on, and
AO's "data, not instruction" marking plus an explicit byte cap are the details to copy.

## 5. Outside this slice

Switching harness on a task already dispatched; cross-machine switching; per-pane harness
defaults; auto-selecting a harness by capability; the raw-terminal detector (slice 9); iPhone
parity for the switch beyond what falls out of the shared client.

## 6. Open items carried in, already measured by victoria

- OpenCode connects and authenticates but its configured provider returns "Your GLM Coding
  Plan package has expired". The app surfaced the agent's own error correctly — no defect,
  recorded so nobody re-diagnoses it.
- The restricted-vocabulary scan has been unverified for several merges (hosted jobs fail
  before any step; the private input is unavailable locally). Not this slice's fault, but do
  not report it as green.
