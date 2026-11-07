# ADR 0152: A pairing shows a short code on both devices

- Status: accepted. Extends ADR 0128 (pairing is decided in the app on the Mac).
- Date: 2026-11-07

## Context

The Mac decides a pairing (ADR 0128), and what it shows the person is the name the new device gave
itself and the fingerprint of its key, the SHA-256 of 32 bytes in 64 hexadecimal digits. The phone
showed the same fingerprint, but nothing told the person to compare them, and nobody compares
sixty-four digits. A request that comes from another program than the person's own phone (one that
read the pairing code, or a script that opened a window itself) looks like the request the person
is waiting for: the name is whatever the sender wrote.

## Decision

- **A short code**, the first twelve digits of the fingerprint in capitals, in three groups of four
  (`E3B0 C442 98FC`), is what the new device and the Mac show, large, while the Mac waits for the
  person. It is a function of the new device's key alone, so the two screens show the same code
  exactly when they speak of the same key. `soyeht_protocol::pairing_link::comparison_code` and
  `PairingCopy.comparisonCode` compute it, with the cases in `contracts/vectors/pairing-comparison.json`.
  The Android core hands its screen the code, and the phone, the tablet and the Mac show it.
- **The words say what to do.** The Mac: allow only if your device shows this same code; if it shows
  another code or none, deny. The device: check that the Mac shows this code before allowing it
  there; if it shows another code or none, cancel.
- **No new protocol.** The engine already sends the fingerprint with the request; the code is
  derived from it where it is shown.

## What this does not do

- Twelve digits are 48 bits: enough that a sender cannot grind a key to match a code it has not
  seen (the person's device makes its key as the pairing starts, after the sender has chosen its
  own), not enough to be a long-term identifier.
- The person has to look. A Mac that is asked to allow in a hurry still allows what is on its screen.
