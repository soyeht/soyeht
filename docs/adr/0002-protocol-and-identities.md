# 0002 — Formats and identities for the first slice

Status: accepted for implementation, 2026-09-10.

## Context

Contract 08 fixes envelopes, limits and operations, leaving concrete types to
the schema. The first slice does not implement ACP, tasks or push.

## Decision

JSON Schema 2020-12 within a closed, documented subset. Object definitions,
enums, arrays, references and scalars generate serde, Codable and TypeScript
models. The catalog associates each RPC name with its parameter and result
types. The generator does not fetch files or execute commands specified by
the schema. UTF-8, JSON without non-finite numbers; u64 transmitted as decimal.

Object IDs are opaque UUIDs. Dates use RFC3339 UTC. JSON byte strings
use standard padded base64. Binary terminal data never passes through
base64. Header integers are big-endian. `eventId` identifies durable
events; challenges and snapshot parts are ephemeral notifications,
without database sequence numbers. `hello` receives one welcome result response.

The text signed in hello concatenates UTF-8 `soyeht-hello-v2`, the decoded
32-byte nonce, the device's decoded 32-byte client nonce, canonical UTF-8
engineId UUID (36 bytes), and the UTF-8 token (ADR 0132 added the client nonce
and the engine's answer).
The pairing code is a random 32-byte base64url secret;
the HMAC uses that code's UTF-8 bytes followed by the raw public key to
derive the SHA-256 key, and the raw nonce + UTF-8 deviceName as its message.
All transmitted signatures and proofs use standard base64. The consumed
code and pairing request are stored atomically with the device.

## Rationale

Signed bytes and missing fields cannot depend on the client's language.
Defining these choices once enables interoperable vectors and avoids
maintaining three manual protocols. Contract 08's standard remains intact.
