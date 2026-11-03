# ADR 0134: The Android app is a Compose view over a Rust core

- Status: accepted
- Date: 2026-11-02

## Context

The owner decided that Soyeht gets an Android app, a companion like the iPhone and iPad app
(ADR 0013, 0099): it pairs with the engine on the person's Mac, Windows or Linux machine by link or
QR over the tailnet, and its job is continuity. What is on screen in a terminal pane or an agent
conversation on the computer shows identically on the phone, and the person continues there: types
in the terminal, talks to the agent, answers permissions, switches harness.

The single-source rule (AGENTS.md) is the weight on the choice. Wire values, limits, client policy,
design tokens and vectors come from `contracts/`. The iPhone gets them from `apps/shared`, which is
Swift and cannot be built for Android. The desktop client (ADR 0084) is Rust and already holds the
connection, the replica and every rule the phone needs, but it is local-only: it dials the engine's
Unix socket as the local human. The pairing and the proof of the engine (ADR 0013, 0128, 0132)
exist only in Swift (`PairingSession`, `DeviceIdentity`) and in the engine.

## Options

1. **Kotlin client.** Port `apps/shared` to Kotlin and add a Kotlin output to
   `contracts/generate.py` for wire models, limits and policy. Every rule (snapshot paging, replicas,
   reconnect, Markdown, transcript rows, layout fit, pane close, harness switch) would be a third
   hand-written copy, held to the first two only by vectors, for the largest client in the
   repository (28,000 lines of Swift).
2. **Rust core, Kotlin views.** Reuse `soyeht-protocol` and the desktop's `soyeht-client`
   (no window system in it, ADR 0084) behind UniFFI; Kotlin renders and forwards input.
3. **Cross-platform UI** (GPUI or another toolkit on Android). GPUI has no Android back end; this
   would be a research project, not an app.

## Decision

**Option 2.** Kotlin with Jetpack Compose draws; a Rust core owns the connection, pairing and replica
state; `soyeht-protocol` is the only definition of the wire.

- **The core** is the crate `soyeht-android` (`apps/android/core`, its own cargo workspace, a
  `cdylib` for `aarch64-linux-android` and `x86_64-linux-android`). It depends on `soyeht-protocol`,
  `soyeht-platform` and `soyeht-client` by path. UniFFI (proc-macro form, no definition file) generates
  the Kotlin bindings from the Rust signatures, so the boundary has one definition too. The bindings are
  build output and are not committed.
- **Reuse, not a fork.** `soyeht-client` gains what a remote device needs, in the places the local
  client already has: an endpoint that is a device credential and a WebSocket instead of a socket path
  (the engine maps a text message to a JSON frame and a binary one to the other frames, as
  `transport.rs` of the engine does), and the device's `hello`. The bytes both sides sign
  (`soyeht-hello-v2`, `soyeht-engine-v1`, `soyeht-engine-pair-v1`, the pairing proof) are written once
  in `soyeht-protocol` (`device_auth`) and pass `contracts/vectors/engine-proof.json`, which the engine
  and the shared Swift client already pass. The engine now takes the bytes from there (its own copy in
  `auth.rs` is gone); the Swift copy remains, held to them by the same vectors.
- **Platform seam.** `soyeht-client` reaches the system only through `soyeht-platform`. That crate
  now builds for Android (the kernel is Linux): the process tree and the no-replace rename are the Linux
  ones, and a crash report has no stack frames because Android's C library has no `backtrace`. The
  phone never uses the local endpoint, the service seam or the detached start.
- **Terminal.** The pane is drawn from `soyeht_client::screen`, the screen model of the desktop client
  (the `vt100` crate over the engine's raw bytes, scrollback from `client_policy`), painted on a
  Compose `Canvas` from styled rows; keystrokes are encoded in the core and sent as `pane.input`. The
  options weighed:
  - *Termux `terminal-emulator` and `terminal-view`* (Java; Apache 2.0, an exception to the GPLv3 of the
    rest of the Termux repository). Proven, but a second emulator with its own edge cases beside the
    Swift one (SwiftTerm) and the Rust one: the same bytes could show differently on the phone and at
    the computer, which is the one thing this app is for. It also brings a view that owns its own
    keyboard handling and selection that the core would then have to be reconciled with.
  - *ConnectBot `termlib`* (Compose; Apache 2.0, with MIT `libvterm` through JNI and C++). No published
    release was visible when this was written, and it is a fourth emulator and a second native library.
  - *The desktop's screen model.* Already tested by the vectors and the desktop's tests, identical to
    the desktop window by construction, and licence-free (it is ours and `vt100`, MIT). The cost is a
    renderer and the soft-keyboard bridge, written once in Kotlin. Chosen.
- **Theme.** `contracts/generate.py` writes a Kotlin output of `design-tokens.json` (palettes, radii,
  spacing, type scale), like the Swift and Rust ones; the views use palette roles only and flat pane
  interiors (AGENTS.md, "Design"). Nothing about the wire is generated for Kotlin: the views never see a
  method name, a limit or an error code, only what the core exposes.
- **Credential.** The device key and token belong to the person's phone. The core returns the
  credential after pairing and takes it back at connect; it never writes it, logs it or sends it to the
  log. The app keeps it encrypted with an Android Keystore key that does not leave the device
  (`allowBackup` off, so it is neither backed up nor moved to another phone), as the iPhone keeps it in
  the Keychain "after first unlock, this device only".
- **Identity.** The application id of the Dev profile is `com.soyeht.next.dev.android`; every machine
  identifier of the Android app contains "next" (ADR 0112). `minSdk` 26, `targetSdk` 35, ABIs
  `arm64-v8a` and `x86_64`.
- **Layers.** `apps/android/core` is layer 7, beside `apps/desktop`: it may depend on `soyeht-client`,
  `soyeht-protocol` and `soyeht-platform` and on no other Soyeht crate. The Kotlin app (`apps/android/app`)
  is the thin layer above it, as `apps/ios/Sources` is above `apps/shared`.
- **Gate.** A Gradle build, an NDK build of the core for two ABIs and an emulator are not cheap. The
  `android_app` area is full-tier only, like `mac_app` and `ios_app`: `android-core:fmt`, `:clippy` and
  `:test` (the Rust core), `android-app:build` and `android-app:test` (`scripts/check_android.sh`, the
  libraries, the bindings, the debug APK and the unit tests of the app). The fast tier keeps what is cheap and
  always runs (layers, generated outputs); a change to a crate the core shares is covered by that crate's
  own area. The emulator run is manual (`docs/apps-android.md`).

## What this does not do

- The app does not scan a QR with the camera in this phase: the pairing link is pasted or opened
  (`soyeht-next://pair?...`, an intent filter of the app).
- It has no push notification, no editor, writing or web pane (they show "Open on the computer", as the
  iPad does), and no board layout for tablets. They follow when the rest has been used.
- It does not move Swift's copy of the signed bytes to the shared functions.
