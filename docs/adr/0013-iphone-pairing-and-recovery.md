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

The iPhone screen first accepted no remote clipboard or terminal-triggered link opening. The
2026-10-19 decision note below changes that, because a terminal that cannot open a link or take
a copy is not a remote the owner can work from: links open on a tap, and a program may write
to the clipboard only when the person turns that on.
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

## Decision note (2026-10-19)

The terminal on a paired iPad or iPhone is a remote for the Mac's pane, so it changes
three of the rules above.

- **Resize.** The contract still lets any human client send `pane.resize`, and the last one
  wins. A device that only opens a pane must not reflow the Mac, so it no longer sends a
  size when a pane is shown or its view changes size. It lays the terminal out at the grid the
  pane already has; a wider pane scrolls sideways. "Fit to this screen" is the person's
  explicit choice to size the pane to the device; choosing "Use the Mac's size" afterwards asks
  for the size the pane had before the fit. The mode lasts for that pane until the app closes
  (`TerminalSizePolicy`, `TerminalSizeModeStore`). The engine is unchanged.
- **Clipboard.** A program in the terminal may write text to the device's clipboard (OSC 52,
  up to 1 MiB, UTF-8) only when the person turned on "Let programs copy to the clipboard" in
  Settings, which is off by default, because Universal Clipboard can carry the write to other
  devices. A write is taken only while the app is active and the terminal is on screen, never
  when a recorded screen is replayed, and it shows "Copied by the terminal". A program never
  reads the clipboard. The person copies a selection and pastes with the system gestures or
  Command-C and Command-V.
- **Links.** A link activated in the terminal opens in the system browser, except an address
  on the Mac itself, which opens in a web pane on the Mac (`WebLinkPreferences`). Only `http`
  and `https` addresses open.

The text size is one device setting between the limits of `paneLayout` in
`contracts/design-tokens.json`, changed by pinch, Command-plus, Command-minus,
Command-0 or the terminal menu.
