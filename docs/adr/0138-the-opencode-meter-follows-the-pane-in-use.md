# ADR 0138: The OpenCode meter follows the pane in use

- Status: accepted. Extends ADR 0133 (the OpenCode meter follows the provider OpenCode is set to).
- Date: 2026-11-04

## Context

ADR 0133 chose the provider from OpenCode's own files: the `model` of its configuration, else the
model it used last. Those files say what a new session starts with. A Soyeht pane can be moved to
another model with `pane.config.set`, and then the ring showed the plan of a provider the person
was no longer using. Measured on the owner's machine: the configuration names an expired GLM
Coding Plan, the person works on the Go subscription and moves the pane to a Go model, and the
ring kept answering `plan_expired` until the configuration file was edited by hand.

## Decision

**The pane is the first thing the meter follows.** The order is: the provider of the model the most
recently active OpenCode pane uses, else the `model` of OpenCode's configuration, else the model it
used last. With no OpenCode pane (or one that has no model yet) nothing changes from ADR 0133.

**The engine copies it under the Core lock.** `usage_refresh_prepare` and the background schedule
read, from the open panes of the OpenCode harness, the current value of the model selector (the
option the clients' policy calls the model: `ConfigOption::is_model`, never a typed option name) of
the pane with the latest update, and hand it to the readers as a `ReadHints` value. `soyeht-usage`
stays a pure, bounded crate with no engine dependency: a hint is data in `ReadContext`, and a
reader that does not need it ignores it. The hint names only a provider; a model with no
`provider/` part is no hint.

**A change is read at once, within the limit of a click.** When a pane commit changes the model of
an OpenCode pane, the engine wakes the usage schedule, which compares the provider the last read
followed with the one the panes follow now and reads OpenCode ahead of its five-minute interval,
no sooner than 30 seconds after its last attempt (the limit of a click). The read is the ordinary
background read: off the Core lock, with the same cooldowns, committed under the lock. When the
followed provider changed, the reading the ring shows belongs to another provider, so a failed
read of the new one is shown, not hidden behind the old reading as a first failure would be.

**A rejected Go login is shown.** HTTP 401 from the Go usage endpoint used to leave the device
activity on the ring, which hid that the person has to sign in again. It now ends the chain with
`login_needed`, as an expired plan ends it with `plan_expired`. HTTP 403 (no Go subscription) and a
missing login still leave the activity.

## Consequences

- `Planner::refresh` takes the hints of the refresh; `ReadContext` carries them to every source.
- `RefreshPolicy::changed_due` is the one place that limits a read caused by a change of what a
  reader follows.
- No wire change: the reasons and sources are the ones of ADR 0133.
- Only the OpenCode harness's own panes count. Another harness that runs OpenCode's models is not
  followed.
