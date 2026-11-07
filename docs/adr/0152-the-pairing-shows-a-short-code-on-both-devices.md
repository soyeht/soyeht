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

- **A short code**, the first sixteen digits of the fingerprint in capitals, in four groups of four
  (`E3B0 C442 98FC 1C14`), is what the new device and the Mac show, large, while the Mac waits for the
  person. It is a function of the new device's key alone, so the two screens show the same code
  exactly when they speak of the same key. `soyeht_protocol::pairing_link::comparison_code` and
  `PairingCopy.comparisonCode` compute it, with the cases in `contracts/vectors/pairing-comparison.json`.
  Both read bytes and accept only a fingerprint that is plain ASCII hexadecimal digits from end to
  end, so a combining mark or a multi-byte character gives no code in either, never a code in one.
  The Android core hands its screen the code, and the phone, the tablet and the Mac show it.
- **The words say what to do.** The Mac: allow only if your device shows this same code; if it shows
  another code or none, deny. The device: check that the Mac shows this code before allowing it
  there; if it shows another code or none, cancel.
- **No new protocol.** The engine already sends the fingerprint with the request; the code is
  derived from it where it is shown.
- **One approval on the Mac.** `PairApprovalView` is the only place a request to pair is allowed
  or denied: the bar over the window and the pairing sheet both show it. It shows the code and the
  words beside Allow, and a request whose fingerprint has no code can only be denied.

## What this does not do

- Sixteen digits are 64 bits (the first decision was twelve, 48 bits): enough that a sender cannot
  grind a key to match a code it has not seen (the person's device makes its key as the pairing
  starts, after the sender has chosen its own), not enough to be a long-term identifier.
- The person has to look. A Mac that is asked to allow in a hurry still allows what is on its screen.
- It does not defend against a malicious app that runs the whole pairing on the person's own
  phone. That app holds the code, makes its own key and shows its own matching code on the phone it
  runs on, so the two screens agree and the person allows an app they did not mean to trust. The
  code shows only that the two screens speak of the same key, not that the app that made the key
  is Soyeht.
