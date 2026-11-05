# ADR 0138: The Android app reads the pairing code with ZXing, and the core judges what it read

- Status: accepted
- Date: 2026-11-04

## Context

ADR 0134 left the camera out of the first Android phase: a phone pairs by a tapped or pasted link. The
iPhone scans the QR code the computer shows (`QRScanner`, VisionKit), and the person at the computer expects
the same on Android: point the phone, confirm, done.

## Options

1. **ML Kit barcode scanning** (bundled model). Accurate and fast, but a Google library: the unbundled
   variant needs Play services (absent on some phones and on the emulator image the app is tested on), and
   the bundled variant adds a native model to every ABI of the APK.
2. **ZXing core** (Apache 2.0). A pure Java decoder of about 550 KB, with no network, no Google service and
   no native code. CameraX feeds it the brightness plane of each frame. It reads a QR code in any
   orientation and, with the inverted picture tried, light on dark too.

## Decision

**Option 2.** `QrDecoder` (`apps/android/app`) reads a QR code from a frame; CameraX shows the picture and
hands frames over (`ScanScreen`). The camera permission is declared (the camera is not required: a phone
without one still pairs by link) and asked for only when the person taps Scan.

What the code says is not judged in Kotlin. The core exports `pairing_offer`, which runs the parse
`Core::pair` runs (`soyeht_protocol::pairing_link`, whose cases are `contracts/vectors/pairing-link.json`).
A tapped link, a scanned code and a pasted link are accepted or refused by that one rule: a text of another
product, a link of an engine that cannot prove itself, an address outside loopback and the tailnet. A
scanned code the parser refuses is said in the parser's words on the scan screen and the camera keeps
looking; the same code is not reported again within a few seconds. A code it takes asks the person first,
naming the address, exactly as a tapped link does (ADR 0129): a QR code on a wall is also input from
outside.

The address the confirmation names is now the parser's (`PairingOffer.address`), where it was the query
parameter read in Kotlin.
