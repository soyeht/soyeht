# ADR 0128: Pairing is decided in the Soyeht app on the Mac

- Status: accepted
- Date: 2026-10-31, amended 2026-11-01

## Context

ADR 0013 gives a paired iPhone or iPad the powers of the person at the Mac, so that it can type
into terminals and drive agents. Pairing a device is the one thing that was meant to stay with the
person looking at the Mac: a device is approved there (contract 08 §2.2).

The engine did not enforce that: `device.pair.start`, `device.pair.decide` and `device.revoke`
accepted any human, a paired device included. Restricting them to the local human closes the
phone's own connection, but not the machine: a paired device can open a terminal, and a program in
that terminal reaches the engine's local socket as the local human.

## Decision

- A paired device may only revoke itself (forgetting the Mac from the phone is its own choice). It
  cannot open a pairing window, decide a request or revoke another device.
- `device.pair.start` needs the local human. The command line may start a request.
- `device.pair.decide` and revoking another device need the **signed Soyeht app** on a system that
  can tell it from other programs. The engine already tells them apart on macOS (the code signature
  of the peer of the local socket, `ui_attestation`, the same rule as the pane-state experiment
  methods). There, a shell, the command line or a script on the local socket, including a terminal
  that a paired device opened, is refused (`forbidden_role`); only the Mac app approves a pairing
  or revokes a device.
- On Windows and Linux there is no such attestation yet. There, **any process of the same user on
  the local socket counts as the computer, including a terminal that a paired device opens**, until
  the apps for those systems can prove they are the official app. The rule for those systems is the
  local human's, as before.
- One function, `ui_attestation::available`, says whether the system has attestation. The rule is
  written once, in `Core::authorize_device_management`, and reads that function; nothing else
  differs by system.

Every other power of ADR 0013 is unchanged. A refused call answers `forbidden_role`. The Mac app
approves through its own connection, so the person can still pair a phone; an unsigned build of the
app (an ad-hoc signature) cannot approve a pairing, as it cannot change the experiment consent.
