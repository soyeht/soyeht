# ADR 0162: Approving a pairing and revoking a device need the signed app on every system

- Status: accepted for the rule, amended by ADR 0163; open for how the Windows and Linux apps prove themselves
- Date: 2026-11-13

## Context

ADR 0128 made `device.pair.decide` and the revocation of another device the signed Soyeht app's
alone, where the engine can tell the app from other programs: on macOS, by the code signature of
the peer of the local socket (`ui_attestation`, the rule of the `pane.experiment.*` methods). On
Windows and Linux it left a hole on purpose: there "any process of the same user on the local
socket counts as the computer, including a terminal that a paired device opens", until those apps
could prove they are the official app.

That hole is the whole defect on those systems. A pairing gives a device the powers of the person at
the computer (ADR 0013). A program in a terminal, which a paired device can open, reached the local
endpoint as the local human and could approve its own next device, or revoke the owner's, with no
person involved. The `pane.experiment.*` methods never had this hole: `requires_signed_ui` in `dispatch.rs` is the gate
that keeps them closed, and outside macOS no local client counts as a signed UI, so the engine refuses
them there (ADR 0085).

## Decision

- `device.pair.decide` and `device.revoke` of another device need a connection the engine has
  attested as the signed app (`Connected::signed_ui`), on every system. There is no system on which
  a local human counts as the app by default. `Core::authorize_device_management` no longer reads
  `ui_attestation::available`.
- Where the engine cannot attest anything (Windows and Linux today), the two methods are refused,
  exactly as `pane.experiment.*` is. The answer is the existing `forbidden_role`. Its message says
  that the Soyeht app decides, and on a system with no attestation that the app cannot yet prove
  itself to the engine there, so no pairing is approved and no device is revoked until it can.
- Amended by ADR 0163: this ADR left `device.pair.start` open to a local human, a shell included. It
  needs the signed app too.
- Unchanged: a paired device may revoke only itself; every refusal is `forbidden_role`; the command
  line and the MCP program offer no way to decide or revoke, and the engine refuses a raw call from
  either.
- `ui_attestation::available` stays for the one other rule that still branches on it, the trust of a
  project folder (ADR 0150), which is a separate decision. That method keeps the old escape on Windows
  and Linux (any local process of the user counts as the app); closing it is an open follow-up.

## Consequences

- A computer that runs Windows or Linux cannot approve a pairing until its app can present a proof.
  Nothing legitimate is lost today: neither app has a pairing view, and the command line has no
  `decide`. The way to pair a phone with such a computer is blocked by design until the question
  below is answered.
- The tests that stood for the app through the `test-hooks` marker (`test-attest-local-ui`) are the
  same on every system now; none of them reads the host's operating system.

## Open question: how the Windows and Linux apps prove themselves

The engine already has the seam (`ui_attestation::signed_ui_peer`, one function whose answer
decides `signed_ui`); the Mac side reads the signature of the socket's peer. Neither other system
has an answer that the existing experiment rule or ADR 0085 gives, so this ADR does not invent one:

- **Windows.** The local endpoint is a loopback TCP port guarded by a secret file (contract 08
  section 2.5), not a pipe, so the peer is found from the connection's port (the owning process of
  the TCP row) and its image is checked with `WinVerifyTrust` for an Authenticode signature of
  Soyeht's certificate. Open: the certificate the release uses (the owner is obtaining it), whether to
  pin its subject or a hash, and how the check avoids a process that was replaced after the lookup.
- **Linux.** There is no operating-system code signing. The peer is found with `SO_PEERCRED` and
  `/proc/PID/exe`, and could be required to be the installed app under a directory the user cannot
  write. Open: a same-user process can still launch that binary under a debugger or a preload, so
  this raises the cost and is not proof. The alternative is that approval on Linux needs the
  person's own action that no process of the user can fake, such as a code typed in the app that the
  engine shows only to it. That is a product decision, not an implementation detail.
