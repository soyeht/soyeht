# ADR 0081: Agent sign-in, account switch and sign-out

- Status: accepted
- Date: 2026-10-07

## Context

On a test Mac a person switched their Claude account from an agent pane. The
app called `pane.auth.start`, the browser opened the sign-in page, and after
signing in the browser was redirected to `http://localhost:<port>/callback`
and showed that it could not connect. No harness process was running.

The sign-in terminal runs the harness's own login command (for Claude,
`auth login --claudeai`), which owns a localhost listener until the redirect.
Tests against the real command show that it stays alive for as long as it is
left alone and ignores stray keystrokes, escape replies and EOF. The engine
never closes it except through the person (`pane.close`) or a completed
sign-in. What does end it is losing its supervisor: a PTY child receives
SIGHUP when the supervisor that holds the master exits. An agent adapter dies
with the same supervisor, which matches a machine with no harness process.
Three gaps made that, and every other early ending, a dead end:

- When the adapter process stopped while a sign-in ran, the engine stopped
  polling the pane, so a sign-in that finished later was never noticed and the
  agent never restarted.
- A sign-in terminal lost to a supervisor restart kept naming a process the new
  supervisor did not have, so every output poll failed at that pane and the
  panes sorted after it, among them the agent that would report the sign-in,
  were not polled again.
- Nothing told the person whether a sign-in was running, finished or lost, and
  there was no way to cancel it.
- A signed-in pane could not change account at all, and had no sign-out.

Separately, the Mac client retried a refused connection every second for ever,
and a second web host claim was refused as `unauthorized`, which hid the cause.

## Decision

The sign-in stays a supervisor-owned terminal. The engine publishes its state
on the agent pane (`agent.login`), reports every ending with a reason, and
keeps polling a stopped agent so that a completed sign-in restarts it.

`pane.auth.start` also works on a signed-in pane with an idle session
(`agent.reauthMethods`). It is then an account switch: the session is
untouched until the login exits 0, the agent restarts and reopens the same
session (`session/load`), and a failed, cancelled or lost switch leaves the
account as it was. `pane.auth.cancel` stops a running sign-in. `pane.auth.logout`
sends ACP `logout` to an agent that advertises `agentCapabilities.auth.logout`
and restarts it. Both refuse while a turn or configuration change runs.

Refused handshakes are logged with the declared client label and the engine's
fixed reason (`engine.handshake_refused`). A second web host claim is refused
as `web_host_unavailable`. The shared client reconnects with a doubling wait
(1 s to 30 s) and, when the host claim is refused, connects without it.

## Consequences

- A sign-in that dies with the supervisor cannot be kept alive; the person
  sees why and starts it again from the same card.
- The engine, CLI, Mac and iPhone/iPad show one state for sign-in, switch and
  sign-out, from `apps/shared`.
- Protocol version stays 1; the additions are optional fields and two methods.
