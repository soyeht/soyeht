# ADR 0128: Pairing and revoking other devices are decided on the Mac

- Status: accepted
- Date: 2026-10-31

## Context

ADR 0013 gives a paired iPhone or iPad the powers of the person at the Mac, so that it can type
into terminals and drive agents. Pairing a device is the one thing that was meant to stay with the
Mac: a device is approved on the Mac, by the person looking at it (contract 08 §2.2).

The engine did not enforce that. `device.pair.start`, `device.pair.decide` and `device.revoke`
accepted any human, so a paired device could open a pairing window, approve a request for another
device and revoke other devices. Control of a device would then outlive its revocation: whoever
held it could pair a second device first.

## Decision

`device.pair.start` and `device.pair.decide` require the local human (`human_local`). `device.revoke`
requires the local human too, except that a paired device may revoke itself: forgetting the Mac
from the phone is the person's own choice and takes nothing from anyone else.

Every other power of ADR 0013 is unchanged. A refused call answers `forbidden_role`. The Mac app
and the command line, which are local, are not affected; no client of a paired device calls these
methods.
