# ADR 0078: Tailnet listeners follow the interfaces

- Status: accepted (supersedes ADR 0015)
- Date: 2026-10-03

## Context

ADR 0015 made the engine exit when its tailnet addresses changed, so launchd
would start a new engine on the current addresses. The first lookup still used
`tailnet_addresses().unwrap_or_default()`: when it failed, the engine listened
on loopback only, wrote nothing to the log and told no client. Phone pairing
then failed with no signal to anyone, and a lookup that kept failing never
caused the restart.

## Decision

- Loopback always listens, so local clients keep working.
- The engine looks up the tailnet addresses at start and every two seconds
  (`network::TailnetListeners`). A new address gets a listener; a removed
  address loses its listener. The engine does not restart, and live
  connections on other listeners are not touched.
- A failed lookup keeps the running listeners: it is not evidence that an
  interface went away (as in ADR 0015).
- Reachability is a contract value, `EngineReachability {remote, reason?}`,
  with the closed reasons `tailnet_address_not_found`, `tailnet_lookup_failed`,
  `tailnet_listen_failed` and `loopback_only` (fixed `--listen` sets).
  `engine.status` and `device.pair.start` return it, and the transient event
  `engine.reachability_changed` announces each change.
- The log has one structured line per change: `engine.tailnet_unreachable`
  with the reason, `engine.tailnet_reachable` with the listener count, and
  `engine.tailnet_lookup_failed`, `engine.tailnet_listen_failed` or
  `engine.tailnet_listener_stopped` with the I/O error kind. Repeats of the same
  state are not logged again. The log never carries addresses or interface
  names.
- The pairing link uses the newest tailnet listener, or loopback when none
  runs. The Mac pairing sheet shows "Not reachable from your devices: …" and
  asks for a new code when the engine becomes reachable. On a device, the phone
  refuses a loopback link with the same text instead of failing later.

## Consequences

The tailnet appearing after login no longer restarts the engine, and a
broken lookup is visible in the log, the status and the pairing sheet.
`engine.network_changed` is no longer logged. Explicit `--listen` addresses
keep the fixed configuration used by isolated tests.
