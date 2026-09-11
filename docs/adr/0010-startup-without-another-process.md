# ADR 0010 — Startup without querying another process

Status: accepted for slice 1.

During real installation under the service manager, calling the network utility
before opening the socket left the engine waiting indefinitely.
The engine now queries the system interfaces directly with `getifaddrs`,
without starting another process.

On Mac, it selects active `utun` interfaces with addresses in the IPv4 range
100.64.0.0/10 or IPv6 prefix fd7a:115c:a1e0::/48. Physical interfaces,
LAN addresses and wildcard addresses are excluded. The local listener remains
available without the tailnet. A test covers exclusion of those addresses.

The installer declares the service ready only after its socket accepts a
connection, with a fifteen-second deadline. If the process is not ready,
it returns an error and points to the private log; it does not touch other services.
