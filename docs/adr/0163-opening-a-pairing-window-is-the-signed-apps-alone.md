# ADR 0163: Opening a pairing window needs the signed app on every system

- Status: accepted; amends ADR 0128 and ADR 0162
- Date: 2026-11-13

## Context

ADR 0128 and ADR 0162 left `device.pair.start` open to any local human, a shell included, on the
ground that a window "opens a request and shows a code, and nothing more": the person still has to
see the request in the Soyeht app and approve or reject it.

That ground is too weak. A program of the same user (a shell, a script, the command line, a terminal
that a paired device opened) can open a window, then pair a device of its own through `pair.begin` and
`pair.prove`, and the app shows an approval prompt with the device name the attacker chose. The
program cannot approve it, but the prompt is the whole interface of the decision, and a name such as
"Caio's iPhone" invites a tap. The comparison code of ADR 0152 does not help: the attacker's own
device shows it. Letting any local program put a prompt of its own choosing in front of the person is
the same defect as letting it decide.

## Decision

- `device.pair.start` needs a connection the engine has attested as the signed app
  (`Connected::signed_ui`), on every system, exactly as `device.pair.decide` and the revocation of
  another device do (ADR 0162). `Core::authorize_device_management` has one rule for the three
  methods: a local human counts only when the connection is the signed app. A paired device is still
  refused the window, the decision and the revocation of another device, and may revoke itself.
- A refused call answers `forbidden_role`, opens no window and records no result, so a replay with the
  same idempotency key is refused too. The message says that the Soyeht app starts a pairing; on a
  system where nothing can be attested, that the app cannot yet prove itself there.
- The command line has no `device pair-start`. The command could only ever answer with the refusal
  (the command line is never the signed app), so a command that always fails would only mislead; the
  command line already has no way to decide or to revoke, and now none to open a window either. The
  Mac app starts a pairing through its own connection (`device.pair.start` from `EngineClient`).
  `soyeht device list` stays: it reads and changes nothing.
- Tests and the headless emulator run of `docs/apps-android.md` stand for the app with the
  `test-hooks` marker `test-attest-local-ui`, as they already did to decide a pairing; the marker is
  checked on every local connection, so a test that starts a window needs it before it connects.

## Consequences

- On Windows and Linux no pairing can be started, as none can be approved, until the apps can prove
  themselves (the open question of ADR 0162). Neither app has a pairing view today, so nothing
  legitimate is lost.
- A developer pairs a phone only from the signed Mac app (`docs/dev-local.md`). An engine run as
  built by `cargo build` cannot start a pairing on macOS either; it says why in its refusal and logs
  one `engine.attestation_unavailable` line.
- What this does not stop is unchanged from ADR 0128: a program of the same user that writes the
  engine's database or the app's files.
