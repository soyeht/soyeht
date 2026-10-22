# ADR 0100: A protocol sign-in is remembered for the harness

- Status: accepted
- Date: 2026-10-20

## Context

ADR 0098 made a pane remember the protocol sign-in method the person pressed and send
`authenticate` to every new agent process of that pane, because some harnesses ask for it from
every process. The choice belonged to the pane, so every new pane of such a harness, and every
switch into it, needed the person to press the sign-in again. A switch candidate had no
sign-in step at all: it failed with "The selected harness requires sign-in".

## Decision

The choice belongs to the harness, in the profile's database (`harness_sign_in`, migration
031: the harness id and the method id, nothing else). It replaces the pane's `agent_sign_in`,
so there is one record and one rule.

- Pressing a protocol sign-in method remembers it for the pane's harness, in the same commit
  that sends `authenticate`.
- Every new agent process of that harness is sent `authenticate` with it once initialized,
  before the capability probe or the session: a new pane, a restart, and the candidate of a
  harness switch (`switch:auth`, before `session/new`). The rule is
  `agents::harness_sign_in::method_to_send`, used by both paths.
- It sends only a method the person pressed, and only while the agent still offers it as a
  protocol (`agent` type) method. Terminal methods are untouched.
- A refused `authenticate` forgets the choice: the pane asks for sign-in again, and a switch
  candidate fails as before. So does signing out of that harness. The write travels with the
  agent change (`Changes::sign_in`) through the store path the engine already uses, and the
  lookup is one primary-key read where the engine already reads the probe cache.
- Only the method id is stored and logged, never a credential.
- Migration 031 moves the choice of each open pane to its harness and removes the pane's copy.

## Consequences

- One press per harness and profile survives new panes, switches, engine restarts and
  supervisor restarts. A harness whose account is gone asks once more, then forgets.
- Signing out in one pane forgets the choice for every pane of the harness; the other panes'
  running processes keep their own state until they restart.
- `scripts/e2e_real_harnesses.py` no longer excludes a harness confirmed with
  `--confirm-sign-in` from the switch ring.
