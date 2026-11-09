# ADR 0153: The pairing QR is not a link

- Status: accepted. Extends ADR 0129 (the ways in from outside the Mac), ADR 0132 (the engine
  proves itself to a paired device).
- Date: 2026-11-06

## Context

The pairing window showed a QR code of the pairing link, `soyeht-next://pair?...`, which carries
the engine's address, its key and the one-time code. The phone's system camera reads any QR code and
offers to open a link in the app that has claimed its scheme. A custom scheme is not exclusive: more
than one app on a phone can register it, and nothing tells the system which is the person's Soyeht.
An app that has claimed `soyeht-next` therefore received the code, the address and the engine key
when the person pointed the system camera at the Mac, and could open its own pairing request while
the person was waiting for one on the Mac and would likely approve it (ADR 0128 asks the person
to look at the request, which names a device and a fingerprint, not an app).

## Decision

- **The QR code carries `SOYEHT-PAIR 1 ` followed by the fields of the link**, not the link. The text
  is not a URL: the system camera shows it as text and hands it to no app. The number is the
  version of the grammar; a prefix of another case or number is refused. The grammar, the
  address rules and the key rules are the link's, in one place (`soyeht_protocol::pairing_link`, the
  Swift `PairingLink`), and `contracts/vectors/pairing-link.json` holds cases for both forms.
- **The scanner in each app reads both forms.** The iPhone and iPad scanner, the Android scanner
  and the paste field accept the QR text and the link, so a Mac or a printed code of the earlier
  form keeps working. The Mac and an older engine: the app draws `qr` when the engine sends it and
  `uri` otherwise.
- **The engine sends `qr` beside `uri`** in the answer of `device.pair.start`. `uri` stays what the
  command line prints and what a person can paste or tap; a tapped link is held for the person's
  answer as before (ADR 0129, rule 4).
- **Not done here:** nothing stops an app from claiming the scheme for the link itself. A tapped link
  still asks "Pair with the Mac at `<host:port>`?" and the Mac still has the last word. A comparison
  code shown on both devices, which would let the person see that the request is theirs, is a
  separate decision.

## Consequences

- Pointing the system camera at the pairing QR code shows text and offers to copy it; the person
  opens Soyeht and uses its scanner. The message that guides pairing says so.
- The system camera may also offer Search or Copy on that plain text, and the text carries the live
  one-time code, the address and the engine key. The code expires in 10 minutes and works once, so
  a copy or a search that leaks it is useful for a short time only.
- A QR code made by an engine from before this ADR is still read.
