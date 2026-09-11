# ADR 0015 · Tailnet after login

Status: accepted. Scope: slice 1.

At reboot, there is no guarantee that the tailnet interface appears before the
engine's LaunchAgent. Querying it only once left the engine restricted to loopback
until manual intervention. This impaired recovery from the iPhone.

In the default configuration, the engine queries interfaces every two seconds.
A change in the address set causes a normal exit; launchd, with KeepAlive, starts
another instance that opens the current addresses. `ptyd` does not restart.
The new engine adopts the shells, and clients repeat snapshot and subscription,
using the same mechanism as the restart acceptances.

This choice uses the already tested recovery path. It creates no other socket
owner and does not change transport. A network fluctuation may cause an additional
engine restart; the log records `engine.network_changed` without copying addresses.
A query error does not count as an interface disappearing.

Explicit `--listen` addresses retain the fixed configuration used in isolated
tests. The Dev installer uses automatic configuration. Interface and range checks
continue refusing physical interfaces, LAN and wildcard addresses. The test covers
late appearance, removal, different ordering and transient errors, without changing
any interface on the machine.
