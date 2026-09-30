# ADR 0013 · iPhone, pairing and recovery

Status: accepted. Scope: slice 1.

The iPhone app uses the same Swift package and generated types as the Mac.
Remote transport uses WebSocket; text contains JSON and binary frames retain
the common header. The QR parser accepts only the product scheme, three unique
fields, a valid port and a literal tailnet or loopback address. The engine
identity received on connection must match the QR.

The iPhone creates an Ed25519 key. The HMAC challenge includes the code, public
key, challenge and name shown on the Mac. The next connection signs the nonce,
engine and token. The credential and key stay in Keychain, accessible only after
the device is unlocked and without migration to another device. Nothing goes into
preferences, connection URLs or logs. The code appears only in the QR and pairing
request. The camera uses the native scanner; devices without that scanner can
open or paste the QR link.

Approval consumes the code in the same transaction that creates the credential.
A new attempt with that code is refused. The original attempt retains its receipt
until the pairing deadline to recover a lost response. The control failed before
the fix and passed afterward; it also verifies that recovery returns the same
credential. The Mac can revoke the device in the pairing window, with confirmation.

On entering the background, the iPhone closes the connection and removes the visible
screen and list. On returning, it performs the handshake, full snapshot and event
subscription. It shows the list only after completing this cycle. Revocation closes
the connection, and a refused credential requires new pairing. The shell and history
remain on the Mac.

The iPhone screen accepts no remote clipboard or terminal-triggered link opening.
Keyboard input follows the 2026-09-26 decision below. Resize remains an allowed request: the last resize wins. Applying
received dimensions to the interpreter does not generate another request, preventing
contention between the two clients.

Local signing uses automatic development signing. The iPhone build passed with
the existing development certificate and profile. CI builds Mac and iOS Simulator
without signing. Physical reboot, a real camera and switching to the mobile network
remain in the manual procedure for acceptance 7; protocol testing does not replace them.

## Decision note (2026-09-26)

By owner decision, a paired iPhone or iPad is a full remote control of the Mac,
exactly like sitting in front of it. It types into terminals, drives agents,
creates, closes and moves panes and workspaces, and changes settings. The engine
gives `human_device` the same powers as `human_local`. This supersedes the
earlier rule that the iPhone terminal was read-only, which the owner had never
agreed.

Authentication does not change. A device must still be approved on the Mac
during pairing and sign every `hello` with its key. An unpaired, unsigned or
revoked connection gets nothing. Agents keep their own limits.

The phone terminal sends keystrokes through the same `TerminalHost` and
`pane.input` path as the Mac terminal. SwiftTerm encodes the software keyboard,
its Esc, Ctrl, Tab and arrow accessory row, and hardware keys, so there is no
second encoder.
