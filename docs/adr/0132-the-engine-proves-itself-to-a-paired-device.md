# ADR 0132: The engine proves itself to a paired device

- Status: accepted
- Date: 2026-11-01

## Context

A paired device proved itself to the engine with a signature over the connection's nonce, the
engine's id and its token (ADR 0002). The engine proved nothing in return: the id it sent in the
challenge is public (it is in the pairing link), so a server at the address the device dials could
answer as the engine. The transport is a plain WebSocket over the tailnet, with no certificate to
check.

## Decision

**The pairing link carries the engine's key.** The engine has an Ed25519 signing key, made on first
start and kept with its database (the `engine_key` metadata entry). The pairing link adds
`engineKey`, its public key, and the device pins it in its credential.

**Pairing is signed.** `pair.begin`'s answer carries `engineProof`, the engine's signature of the
request, the challenge and the device's key. The device checks it against the key of the link before
it proves the code, so a server that is not the engine the QR named is not paired with.

**Every connection is mutually authenticated.** The device adds a nonce of its own to `hello` and
signs both nonces (`soyeht-hello-v2`); the engine's `welcome` carries `engineProof`, its signature
of both nonces, its id and the device's id. The device closes the connection, with an explanation,
when the proof is absent or wrong. Both nonces are fresh per connection, so an answer cannot be
replayed from another connection. The bytes are in `contracts/vectors/engine-proof.json`, which the
engine and the shared Swift client both pass.

**Old and new meet with a reason.** A device that sends no client nonce is answered
`protocol_version_unsupported` with "update Soyeht on this device"; its reconnect loop shows that
text. A device that finds no proof in `welcome` shows "update Soyeht on the Mac" and stops pairing
or connecting. A credential saved before this ADR has no engine key: the device asks the person to
pair again (`engine_identity_changed`). Nothing falls back to the old handshake.

## What this does not do

It authenticates the engine; it does not encrypt the connection. Confidentiality on the path is the
tailnet's. A channel keyed from the same keys is a separate decision.
