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

**Every connection carries a proof of the engine.** The device adds a nonce of its own to `hello` and
signs both nonces (`soyeht-hello-v2`); the engine's `welcome` carries `engineProof`, its signature of
both nonces, its id and the device's id. The device closes the connection, with an explanation, when
the proof is absent or wrong. Both nonces are fresh per connection, so an answer cannot be replayed
from another connection. The bytes are in `contracts/vectors/engine-proof.json`, which the engine and
the shared Swift client both pass.

The order matters: the device sends `deviceToken` and its own proof in `hello` before it has checked
the engine, because the engine's proof answers that `hello`. The token alone is not enough to connect
(the engine also needs the device's signature, made with a key that never leaves the phone), but a
server that is not the engine does see the token.

A proof that does not verify ends the connection as `not_your_mac`, not as a refusal of the saved
access: the device says "this is not your Mac" and keeps trying with backoff, so a server at a stale
or borrowed address cannot switch a real phone off. Only a refusal by the engine itself (`unauthorized`,
`engine_identity_changed`) stops the reconnecting and asks the person to pair again.

**Old and new meet with a reason.** A device that sends no client nonce is answered
`protocol_version_unsupported` with "update Soyeht on this device"; its reconnect loop shows that
text. A device that finds no proof in `welcome` shows "update Soyeht on the Mac" and stops pairing
or connecting. A credential saved before this ADR has no engine key: the device asks the person to
pair again (`engine_identity_changed`). Nothing falls back to the old handshake.

## The engine key and the database

The key lives and dies with the database (the `engine_key` metadata entry). Restoring a backup of the
database restores the key, and the phones paired in it keep working. Recreating the database makes a
new key, and every phone has to pair again. The upgrade that introduces this ADR is such a break for
the phones already paired: they have no engine key, so each is asked to pair again (see the
changelog).

## What this does not do

**The proof is not tied to the channel.** It shows that the server holds the engine's key; it does not
say that the connection is the one the phone dialed. A program that relays the whole connection between
the phone and the real engine forwards the engine's own proof, passes the check, and sees everything.
Only a server that does not hold the key is stopped. Nothing here encrypts the connection:
confidentiality on the path is the tailnet's, and a channel keyed from the same keys is a separate
decision.
