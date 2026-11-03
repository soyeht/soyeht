# ADR 0136: The window registers the login start and asks the service first

- Status: accepted
- Date: 2026-11-03

## Context

A clean `.deb` install on Ubuntu 26.04 arm64 left a person who opened the app with no way to have
Soyeht start at login but a terminal command (`soyeht service install`): the window offered no
question and no setting (ADR 0121 listed "a prompt in the window" as not done). The window's own
start of the engine (ADR 0102) also never asked a registered service to start, although ADR 0114
and contract 13 section 4 say it should: with the units registered and the engine killed, the
window started a detached engine, which the next restart of the unit then fought over the lock.

`soyeht-client` may depend only on the protocol, the platform and the process crates
(`scripts/check_layers.py`), so it cannot call `soyeht-service`. The Mac app solved the same
problem by running the command line of its own bundle (ADR 0120).

## Decision

**The window runs the `soyeht` beside it, as the Mac app does.** `soyeht_client::login_start`
asks `soyeht --profile P --data-dir D service status|install|uninstall|start` through the same
runner as "Copy diagnostics" (`diagnostics::ask_within`, ADR 0118), within `serviceCommandMs`
(`x-clientPolicy.timeouts`, 60 s, which the Mac's `BundledEngineService` now reads too). It
writes no service file. A window with no `soyeht` beside it (a copy that is not a release) has
no setting and asks nothing.

**It asks once, and keeps a setting.** When the window can start an engine itself (no
`SOYEHT_ENGINE_SOCKET`) and `service status` says nothing is registered, it shows, under the top
bar, "Keep Soyeht running in the background?" with "Turn on" and "Not now", the Mac's words.
"Turn on" runs `service install`. "Not now", and turning the setting off later, are kept in the
device's own choices (`login_start_declined` in `client-preferences.json`), so the question is
not asked again. The Setup panel of Preferences has the switch "Start Soyeht at login" for good.
Nothing is registered unasked (contract 13 section 3): a Windows task is listed and a systemd
unit is a background program the person did not choose.

**Turning it off is `service uninstall`.** The seam has no "unregister and leave running". So the
switch ends the engine and the supervisor and the terminals with them, and says so beside it. The
uninstall leaves the `uninstalled` record of ADR 0131, which an open window takes for an
uninstall by someone else. The window therefore holds `Engine::service_change` while the command
runs: the supervisor starts no engine meanwhile, and when the change ends it takes the record
as its baseline (`Watch::accept_current`), then starts an engine of its own, since nothing is
registered. The record still stops the window from starting an engine after a real uninstall.
A seam command that unregisters without stopping is a follow-up; it was not designed here because
it cannot be checked on the Mac and Windows from this change.

**The start asks the service first.** `autostart::ensure` runs, in order: the engine answers
(done); an uninstall record is new (stop, ADR 0131); the window's `desktop-start.lock`; the engine
answers (done); if `service status` says registered, `service start` and wait for the endpoint;
otherwise, or if `service start` failed, the detached start of ADR 0102.

The contract says the app must not fall back when the service start fails. This keeps that for a
start that was **accepted** and did not bring the engine up in time (an error, no second engine),
and falls back only when the **request itself is refused**: the unit's program is gone, the
service manager does not answer, the command cannot run. In those cases the service is not
starting anything, and a window that shows "the engine is not running" in front of a working
copy is worse than the small risk of a refused request that the manager honours later (contract
13 section 4 keeps both sentences).

## Consequences

- A person who installs the package gets the question at first run and the switch afterwards; the
  `.deb` still registers nothing by itself (ADR 0121).
- With the services registered, the window's start no longer leaves a detached engine beside a
  unit. Two windows still make one decision (the lock).
- A first run costs one `soyeht service status` (a few `systemctl` calls) when the engine is not
  running, and one when the window opens.
- The window does not repair a registration whose programs moved (the Mac does, contract 13
  section 3): a unit whose program is gone is skipped by systemd (ADR 0121), `service start`
  reports it, and the window falls back to its own start. Turning the switch off and on repairs it.

## Verified

On Ubuntu 26.04 (arm64) with the `.deb` built by `scripts/package_linux_rootless.sh`: the first
run asks the question and "Turn on" registers both units (the programs the window had started kept
running, the units were enabled and left idle); the Setup panel shows the switch on, turning it off
removes the units, ends the engine and has the window start one of its own with no "uninstalled"
message, and turning it on registers them again; with the units registered and no engine, a new
window starts the two units and no program of its own (one engine, in the unit's cgroup); a unit
whose program is missing is refused by `service start` and the window starts its own engine and
logs `desktop.service_start_failed`; `soyeht uninstall` with a window open ends everything and the
window says it was uninstalled and starts nothing; `apt remove` follows.

## Not verified

The Windows path (a scheduled task: `schtasks`, the host program, the first-run question in a
Windows window). The code is shared and compiled for the Windows target by the gate; nothing of it
ran on Windows. The Mac was not run either: only the one-line change of `commandSeconds` to the
generated policy value was compiled (`swift build` of `apps/shared`). "Not now" was exercised by the
tests and by the switch's own "off" (which keeps the same choice), not by a click on the button.
