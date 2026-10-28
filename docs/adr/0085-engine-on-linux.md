# ADR 0085: The engine runs on Linux

- Status: accepted
- Date: 2026-10-08

## Context

The product has to run on Windows and Linux as well as macOS. The engine was
written on macOS; most of it already compiled elsewhere because the macOS-only
parts (the signature check of the local peer, the trash, keychain and browser
reads) were behind `cfg(target_os = "macos")`. A first run on Ubuntu 26.04
(arm64) built the whole workspace but failed 130 tests, for three reasons:

1. The local socket refused every client. `ui_attestation::peer_audit_token`
   returned `None` outside macOS, and the accept loop drops a connection that
   has no token, so no client could ever connect on Linux.
2. A new terminal pane ran `/bin/zsh -l` on every system. On a Linux host
   without `zsh` the pane never started; with `zsh` installed and no startup
   files it stopped at the first-run questionnaire.
3. Three tests depended on the host: one expected `/bin/zsh`, one asserted that
   the person's own `PATH` held no `.grok/bin`, and the version-control tests
   need `git` on `PATH`.

## Decision

- **Local peers.** The accept loop still requires the peer to run as the
  engine's own user (`SO_PEERCRED` on Linux, the same call as macOS). Outside
  macOS the audit token is a unit value that carries no identity, and
  `signed_ui_peer` is always false. A Linux client is therefore a local human
  client that is never a signed UI, so `pane.experiment.set`, `.delete` and
  `.export` stay refused there. Nothing weaker than macOS is accepted: the
  consent-changing methods are exactly as protected as before.
- **Default shell.** `soyeht_ptyd::shell::default_login_shell` is the single
  place that names the shell of a new pane. macOS keeps `/bin/zsh -l`. Other
  systems use `$SHELL` when it is an existing absolute path, then the shell the
  account names in the user database, then `bash`, then `sh`, always as a login
  shell.
- **Linux log bound.** `capture_stderr` finds the file behind descriptor 2
  through `/proc/self/fd`, as macOS does through `F_GETPATH`, so the 16 MiB
  engine-log bound of ADR 0082 also holds on Linux. Systems with no way to name
  the file keep the documented "not captured" result.
- **Tailnet interface.** `tailscale0` counts as the tailnet tunnel next to
  `utunN`, so a Linux engine follows the tailnet addresses.
- **Tests.** The tests that read the host now build their own input (`bash` as
  the probe shell, the launch path compared with itself). The check needs `git`
  and says so when it is missing.
- **Check script.** `scripts/check_engine_linux.sh` runs format, lints, build
  and tests; `scripts/vmpal_check_engine.sh` runs it in a VMPal VM from the
  committed tree. Neither is a step of `scripts/check_all.sh`, which stays the
  one merge gate and runs on macOS (`docs/platforms.md` says how to wire them).

## Consequences

The workspace builds, passes `clippy -D warnings` and passes all its tests on
Ubuntu 26.04 arm64. macOS behaviour is unchanged: the same `cfg` branches, the
same shell, the same attestation. The Linux-only differences (no trash, no
keychain, no signed UI) return the existing typed "unsupported" results rather
than failing or being skipped silently.

## Rejected

- Treating every local Linux client as a signed UI. That would let any process
  of the same user change experiment consent, which macOS forbids.
- Reading `$SHELL` on macOS too. The macOS shell is part of the installed
  profile and stays fixed.
