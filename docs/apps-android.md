# Android app in the Dev profile

The Android app is a companion of the engine on the person's Mac, Windows or Linux computer, as the
iPhone app is (`docs/apps-iphone.md`, ADR 0134). Jetpack Compose draws; a Rust core (`apps/android/core`,
exposed through UniFFI) pairs the phone, connects as a device and keeps the replica the desktop client
keeps. The application id of the Dev profile is `com.soyeht.next.dev.android`.

## What it does

- **Pairs by link or by camera.** On the computer choose the pairing window; scan its QR code (the
  camera permission is asked for when you tap Scan, and ZXing reads the code on the phone, ADR 0138),
  paste the link (`soyeht-next://pair?...`) into the app, or open it on the phone. All three go through
  the core's one parser; a code that is not a pairing link is refused in words and the camera keeps looking. A phone that is
  already paired scans from "Pair another" in Settings; the new computer replaces the old
  one only after the person confirms, as for a tapped link. A link that reaches the app from
  outside asks first, naming the address it would connect to (ADR 0129). The app shows the fingerprint
  of the phone's key to compare with the one on the computer, and the person allows access there. The
  phone checks that the engine holds the key the link named before it proves the code (ADR 0132). Both
  must be on the same tailnet; a link names a tailnet or loopback address and nothing else.
- **Draws the approved designs** (`design/soyeht-desktop.pen`), the same way on every screen size. A wide screen
  (600 dp or more, a tablet in either orientation) shows the workspace tabs over the rail of apps and the panes: the
  rail picks what to see (agents, editor, web, terminals) and holds a usage ring for each provider the engine meters
  and the settings; the panes sit side by side on a landscape screen and stacked on a portrait one, and become tabs
  when they do not fit at a readable size (340 x 360 dp). A narrow screen (a phone) shows the list of workspaces and,
  inside one, a segmented switch and one pane at a time. A pane is a raised shell, the only raised surface, holding
  its header (state dot, name, what it runs, the state in words) over a flat interior; the tabs and the top bar's
  buttons are raised too. Colors, radii, spacing, type sizes and the shadows of the raised surfaces are the tokens of
  `contracts/design-tokens.json`; the fonts (Nunito, Inter, JetBrains Mono) are the files the Mac ships, copied into
  the app's resources at build time, and so are the four providers' logos (the shared package's `ProviderIcons.xcassets`, tinted in a palette color as `ProviderGlyph` does). A logo is in each ring of the rail and leads the harness word of the composer's line, by the core's mapping of a harness to its provider; in the e-ink mode they stay, as content.
- **Terminal pane.** The screen is the computer's, drawn from the desktop client's screen model: what
  the computer shows is what the phone shows. The phone never resizes the pane (ADR 0013); a pane wider
  than the phone scrolls sideways. Typing goes through the soft keyboard; a row has Esc, Tab, Ctrl,
  the arrows, Backspace and Enter. A hardware keyboard sends Enter, Backspace, Tab, Esc and the arrows.
- **Agent pane.** The conversation, with an agent's replies and thoughts drawn as the shared Markdown subset
  (ADR 0087, 0139: bold, italic, code, lists, quotes, tables, links, and a Copy button on each code block),
  the prompt (queued while a turn runs), tool calls as ruled rows, and the question the agent waits on (a
  permission), with the options the agent gave: the allowing one in ink, a refusal in red. Under the message
  field a line of mono words names the harness the pane runs (choosing another restarts the pane on
  it and hands the conversation over, after asking, ADR 0140) and sets the session's model, effort and
  permission mode. The usage ring of each provider (ADR 0141) is in the rail.
- **Writes with the pen** (ADR 0157). A stylus touch on an agent pane's message field opens an ink area, draws the
  strokes as they come, and after a 1.2 s pause reads them and inserts the text at the cursor; a scribble over the
  writing takes it away, a scribble over nothing deletes the word before the cursor, and a tap ends the writing. A
  finger still scrolls and types. From Android 14 the platform's own stylus handwriting does it through the keyboard and
  the app does nothing; below it, ML Kit's recognizer reads the strokes on the device with a language model that follows
  the system language, which the person installs from Settings (nothing is downloaded before Install). The ML Kit
  libraries are proprietary Google binaries, free of charge, that the owner accepted on 2026-11-08 (the policy lists
  them as accepted exceptions); ML Kit sends performance and usage metrics to Google, which the Settings row and
  `PRIVACY.md` say, and the strokes and the text stay on the device.
- **Forget this computer** removes the credential. The credential (the phone's key and the token) is
  kept encrypted with an Android Keystore key, in storage that is not backed up and does not move to
  another phone.
- "Forget" (in Settings) asks first. A web, editor or writing pane says it is shown on the computer.

- **E-ink mode** (ADR 0155). On by itself on a Boox (`Build.MANUFACTURER` is `ONYX`) and switchable by hand in Settings. It draws the layout above, unchanged. It uses the e-ink palette and sizes of `contracts/design-tokens.json`
  (`einkPalette`, `eink`): high contrast, an edge on every card, well, pill and modal, 18 sp text, a 56 dp touch
  target, no animation, a reading column on a wide screen, and a terminal whose font grows until the columns fill
  the pane's width (the phone still never resizes the pane). Terminal output is redrawn about four times a second
  and each row is drawn only when it changes. On a Boox the terminal also steers the panel's refresh: the text
  mode by default, the fast mode while output streams, one full refresh about 400 ms after it stops, and a
  "Refresh screen" button. Anywhere else the refresh does nothing.

Not yet: push notifications, new panes
and workspaces, closing a workspace tab, the tasks page, the the context ring and the microphone of the composer,
terminal text size and selection (outside the e-ink mode).

## Build and test

```sh
bash scripts/check_android.sh          # the Rust core for arm64 and x86_64, the Kotlin bindings, the debug APK
bash scripts/check_android.sh --test   # the unit tests of the app, against the core built for this computer
```

They need an Android SDK with NDK 27.0.12077973 (`ANDROID_HOME`), JDK 17, and the Rust targets
`aarch64-linux-android` and `x86_64-linux-android`. The script downloads Gradle 8.10.2 once, into
`.cache/`, and checks its checksum. The core's own format, lint and tests are `android-core:*` steps of `scripts/check_all.sh` in the
`android_core` area, which the fast tier runs when the core, the desktop client or an engine crate they
build on changes; the builds above are the `android_app` area, which only the full tier runs. The app's
`minSdk` is `soyeht.minSdk` in `apps/android/gradle.properties`, which the script uses for the NDK builds too.

`contracts/design-tokens.json` is the source of the app's colors, radii, spacing and type scale
(`DesignTokens.kt` is generated by `contracts/generate.py`); the views use palette roles only and a
pane's interior is flat.

## Trying it on an emulator

Headless, with an engine of its own (never the one you work with):

1. Start `soyeht-ptyd` and `soyeht-engine-test` (`cargo build --features test-hooks`, the program the
   end-to-end suites use) with a short `--data-dir` of their own and `--listen 127.0.0.1:PORT`. Put a file
   named `test-attest-local-ui` in the data directory, so a local client can approve a pairing as the
   signed Mac app would.
2. Start the emulator without a window: `emulator -avd NAME -no-window -no-audio`. Forward the port with
   `adb reverse tcp:PORT tcp:PORT`: the emulator reaches the host's loopback only that way, and a link
   names the loopback.
3. `adb install -r apps/android/app/build/outputs/apk/debug/app-debug.apk`, then open the link with
   `adb shell am start -a android.intent.action.VIEW -d '<link>'` and allow access with
   `device.pair.decide` from a local client.
