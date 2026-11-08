# ADR 0157: The Android app writes with the pen on the message field

- Status: proposed. Extends ADR 0134 (the Android app) and ADR 0155 (the e-ink mode); needs the owner's decision on the ML Kit terms (below) before a release.
- Date: 2026-11-08

## Context

The owner writes on a Boox Note Air 3 C (Android 12, SDK 32) with its pen. Android's own handwriting in text fields
(`View.setAutoHandwritingEnabled`, the keyboard's stylus input) needs Android 14 (SDK 34). The Boox firmware has no
handwriting to text across apps: its note app keeps strokes or a picture. The keyboard (Gboard) has a handwriting
layout, which is the stopgap and is not changed by the app.

## Decision

**The pen writes on the message field of an agent pane.** A stylus touch on the field (`PointerType.Stylus`; a finger
is not read and scrolls and types as before) opens an ink area: the field grows to three touch targets and the strokes
are drawn over it as they come. After a pause of 1.2 s with the pen up, the strokes are recognized and inserted at the
cursor, with a space after a word before it. A scribble over the writing removes the strokes it covers; a scribble
with nothing under it deletes the word before the cursor; a tap ends the writing (and, with no writing, takes the
keyboard). The ink area closes by itself 4 s after the last insert. On a Boox the panel is in its fast mode while the
pen writes and takes one clean refresh when the writing ends (`ScreenRefresh`, ADR 0155). What a stroke is, a tap, a
scribble, what a scribble covers and the text edits are pure code with unit tests (`ink/`); the pointer handling is in
`ui/Writing.kt`.

**Recognition is on the device.**

- **Android 14 and later:** the platform handles a stylus in a text field through the keyboard, and the app does
  nothing (no ink area, no model, no row in Settings).
- **Below Android 14:** ML Kit's Digital Ink Recognition (`com.google.mlkit:digital-ink-recognition` 18.1.0) reads
  the strokes with a language model. The model follows the system language: its tag, then its base language, then
  English (`HandwritingLanguage`), so Portuguese and English are covered, as are the other languages the recognizer has.
- **The model is opt-in, as the dictation engines are (AGENTS.md).** Nothing is downloaded before the person presses
  **Install** in Settings, whose row says where the model is (checking, not installed, installing, installed, failed,
  none for this language) and can remove it. Until it is installed a stylus touches the field as a finger does.
  The state of the model is read from the model manager (nothing is downloaded to read it).

**The license of ML Kit, and what the gate does with it.** The repository is Apache-2.0, and the gate allows only
permissive licenses (ADR 0149). The libraries ML Kit brings are not open source:

| Library | License |
| --- | --- |
| `com.google.mlkit:digital-ink-recognition`, `com.google.mlkit:common` | ML Kit Terms of Service (Google's, proprietary; free to use in an app; no source) |
| `com.google.android.gms:play-services-base`, `-basement`, `-tasks` | Android Software Development Kit License (Google's, proprietary) |
| WorkManager, Firebase components and encoders, Datatransport, OkHttp, Okio | Apache-2.0 |

None is GPL, AGPL or LGPL, and no GPL code is linked: the check `third-party-notices:check` still fails for one. The
proprietary ones are listed in `scripts/third_party/gradle.json` and in `exceptions` of `scripts/third_party/policy.json`,
each with its reason. The gradle part of `third_party_sources.py` now honours `exceptions` as the cargo part does.
**`release_flow.py collect` and `publish` refuse while an exception is in the policy, so no release ships with this until
the owner accepts the ML Kit terms** (or the dependency is replaced). That is the intended consequence, not an
oversight. Also for the review: the SDK carries Google's logging transport (Datatransport), which reports usage
metrics under the ML Kit terms; the strokes are recognized on the device and are not sent by the app.

## Alternatives

- **Gboard's handwriting layout (the stopgap).** Works today, is the keyboard's own, and needs a switch of layout for
  each use; this change does not touch it.
- **Rasterize the strokes and read them with an open OCR engine** (Tesseract, Apache-2.0). Permissive, but OCR reads
  printed text, not handwriting, and would be poor for the pen.
- **No recognizer below Android 14.** Keeps the repository permissive, and leaves the owner's device without the feature.

## Consequences

- A new dependency with a proprietary license: the review is required, and a release is blocked until it is decided.
- The APK grows by the ML Kit libraries; the model is not in it.
- On Android 14 and later the code is not used; the platform's handwriting takes over.
