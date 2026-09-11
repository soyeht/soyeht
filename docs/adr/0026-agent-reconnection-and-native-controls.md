# ADR 0026 · Preserve agent reconnection state and require native proof

Status: accepted for slice 2.

The agent pane snapshot always carries the bounded public authentication
methods read during initialization. The durable login event may precede a
client's snapshot cursor, so relying on that event alone can leave a reconnected
client unable to present the sign-in action. Database version 6 adds an empty
authentication-method list to version-5 pane records; live adoption replaces
that list with the initialized process state before publishing a new change.

The protocol defines the durable `pane.queue.steered` event promised by the ACP
contract. ACP v1 does not define a standard steering or context-compaction
operation. An absent capability is therefore unavailable, rather than inferred
from an agent name or a successful ordinary prompt. Clients disable “Send now”
with the contract tooltip and show context numbers without a compaction action.

Enabling either native control requires a disposable, per-fingerprint probe
that proves the exact operation and a reviewed adapter that defines completion,
cancellation and recovery after an ambiguous disconnect. A timeout or parameter
error remains inconclusive. The protocol version remains 1 because no client has
been released and these changes add snapshot data and an event without changing
an existing method.
