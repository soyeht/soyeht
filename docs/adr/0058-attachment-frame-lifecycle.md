# ADR 0058: Attachment frame lifecycle

- Status: accepted
- Date: 2026-09-13

## Decision

The existing 16-byte attachment subheader carries the raw UUID bytes of
`attachmentId`. An attachment is sent as contiguous, ordered type 3 chunks on
one connection. The JSON message that first references that identifier is the
commit marker: its `AttachmentRef.bytes` must equal the accumulated byte count.
The receiver rejects unknown references, interleaving, duplicate completion,
size overflow and a connection that closes with an incomplete attachment.

The engine validates and owns bytes referenced by a queued agent prompt. It
stores those bytes with the prompt before acknowledging the mutation, then
converts them to the native image content block only when that prompt starts.
Web command images are transient: the host uploads them before its reverse RPC
response, and the engine forwards the same bounded bytes before replying to the
tool caller. No screenshot or picked-element image enters the event log.

## Consequences

The framing stays additive and uses the attachment frame already reserved by
the protocol. Ordered local sockets and WebSocket messages supply chunk order,
so the header does not duplicate an offset. A reconnect must upload unfinished
bytes again. JSON remains the only completion boundary and carries the public
metadata used by generated clients.
