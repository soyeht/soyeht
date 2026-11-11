# Changelog

All notable changes to Soyeht are written here, for the person who uses it. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the version is the one in
`contracts/release.json` ([ADR 0115](docs/adr/0115-one-product-version.md)).

A release's section is drafted from the pull requests merged since the last tag
(`uv run python scripts/changelog.py draft`), edited by a person, and becomes the text of the
GitHub release that the update notice links to (`docs/release.md`). Entries that wait for the next
release go under Unreleased, in the categories Added, Changed, Deprecated, Removed, Fixed and
Security.

## [Unreleased]

### Added

- The Android app can read the pen's handwriting in the message field on devices below Android 14, with Google's ML
  Kit. It is off until you turn on "Handwriting on this device" in Settings. Turning it on starts ML Kit, which sends
  diagnostics and usage metrics to Google (`PRIVACY.md` lists them); what you write is not among the data Google lists.
  The Android app contains proprietary runtime libraries from Google (ML Kit and Google Play services), under Google's
  terms and not open source, and are listed in `THIRD-PARTY-NOTICES.txt`.

## [0.1.0] - 2026-11-04

The first public release.

Push notifications are not part of this release. The iPhone and iPad app does not ask for
notification permission, and your computer sends nothing to Apple's push service. The Mac, Windows
and Linux apps still raise a system notification on your computer when an agent needs you. Push
comes back in a later release.

### Added

- Soyeht for the Mac, for Windows and Linux, and for the iPhone and iPad. The phone and the tablet
  connect to your Mac and work with the same terminals, agents and documents.
- Agent panes, terminals, a code editor, texts, screenplays, presentations and web panes in one
  workspace. Windows and Linux have the same panes, including web panes (on WebView2 and on the
  system's web view), and keep the theme you chose.
- Terminals and agents keep running when you close the window: the engine and the supervisor that
  hold them start at login, and a supervisor that was lost is started again.
- An agent pane runs the version of the agent's command line that you have installed.
- A Copy button on the code blocks of an agent reply.
- The terminal on the iPhone and iPad takes handwriting from Apple Pencil, and the pane board keeps
  its arrangement when the keyboard opens.
- Usage rings for the coding agents you use, including one more provider, shown beside the pane.
- A notice when a newer Soyeht is published. Once a day the app asks GitHub for a signed manifest;
  the request carries no version, system or identifier, you can turn it off in Preferences, and
  nothing is installed without you.
- A notarized disk image for the Mac, a `.deb` and an AppImage for Linux, and a per-user installer for
  Windows. The Mac app starts its own engine from the bundle.
- `soyeht uninstall`, which also ends the engine and the supervisor that the window started, and
  `soyeht diagnostics`; the Mac, Windows and Linux apps can copy a diagnostics report with the
  private details removed.
- A local crash report when the engine, the supervisor or a window fails, and a message that Soyeht
  closed unexpectedly. Reports stay on your machine until you choose to send one.
- Soyeht asks before it closes a workspace, a terminal or an agent pane.

### Security

- Pairing is approved only in the signed Mac app, never from a paired device or a shell, and a tapped
  pairing link asks before it pairs and names the Mac it would replace.
- A paired iPhone or iPad now checks that the server it connects to is your Mac. The Mac signs each
  connection with a key that the pairing code carries. **Every iPhone and iPad paired before this
  version must be paired again**, and the Mac must be updated before the phone is: a phone that finds
  an older Mac says to update it, and one paired before says to pair again.
- The supervisor's socket checks the user on the other end, and a private directory counts as private only
  when you own it. A web page can no longer open a WebSocket to the engine.
