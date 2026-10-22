# ADR 0098: A protocol sign-in is repeated for each agent process

- Status: accepted
- Date: 2026-10-19

## Context

ADR 0066 gives an agent two kinds of sign-in. A `terminal` method signs the person in once:
the harness's own login command stores the account, and every later process finds it. An
`agent` method is a protocol request (`authenticate`) that the person confirms from the pane.

Some harnesses (the Cursor CLI is one) ask for the protocol request from every process, not
once per account: `session/new` fails with the authentication error until that process has
answered `authenticate`, even when the CLI is signed in. The engine started a new process after
the capability probe, after every sign-in and after a supervisor restart, and it sent
`authenticate` only when the person pressed the button. The new process then failed again,
and the pane asked for the same sign-in without end.

## Decision

The engine remembers the protocol method the person chose (ADR 0100 keeps it per harness, in
the store; this ADR first kept it in the pane's agent state). Each new process, once
initialized, is sent `authenticate` with that method before the probe or the session, as a
request of its own purpose (`reauth`). The pane never opens a session on a process that has
not been authenticated.

- It sends only the method the person pressed, and only while the agent still offers it as a
  protocol method. A method the person never chose is never sent, whatever the agent offers.
- A refusal clears the choice and the pane asks for sign-in again. So does signing out.
- Terminal methods are untouched.

## Consequences

- A pane of such a harness needs one press of its sign-in, then survives the probe restart,
  an engine restart and a supervisor restart.
- The choice belonged to the pane, so a new pane of the same harness, and a switch into the
  harness, still asked once. ADR 0100 keeps the choice per harness instead; the rules above
  are unchanged.
- `scripts/e2e_real_harnesses.py --confirm-sign-in HARNESS` presses that button for a harness,
  so the acceptance can exercise it; without the flag the harness is skipped as not signed in.
