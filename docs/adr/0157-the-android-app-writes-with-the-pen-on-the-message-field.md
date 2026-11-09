# ADR 0157: The Android app writes with the pen on the message field

- Status: accepted. Extends ADR 0134 (the Android app) and ADR 0155 (the e-ink mode). The owner accepted ML Kit on 2026-11-08.
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
- **ML Kit is opt-in twice, and off by default.** The default recognition is meant to be the paired computer's
  (designed separately, not part of this change); ML Kit is the on-device alternative. In Settings the person turns
  "Handwriting on this device" on (the row says that it uses Google ML Kit and what ML Kit sends to Google), and only
  then **Install** downloads the language model, as the dictation engines do (AGENTS.md). While it is off the app
  never calls ML Kit (no model check, no download, no recognizer) and a stylus touches the field as a finger does; it
  does so too until a model is installed. Once on, the row shows where the model is (checking, not installed,
  installing, installed, failed, none for this language), names the language of the model in use (which can be English
  when the system language has none), and Remove deletes it. A failed step offers Retry for that step only: a failed
  removal retries the removal, never an install. Turning it off cancels a check or an install in flight, so a late
  result sets nothing (ML Kit cannot stop a download it has begun: that file stays until Remove, which is offered only
  while it is on) and closes the recognizer.
- **Nothing of ML Kit starts while it is off.** The library's own manifest would start it at every launch of the app.
  The app's manifest removes that (see "What ships in the manifest"), and the app starts ML Kit itself
  (`MlKit.initialize`) the first time it needs it after the person turned the recognizer on, or at launch if it was
  already on. All calls go through the small `InkEngine` interface (`MlKitInkEngine` is the one implementation), so a
  unit test counts them with a fake: construction, refresh, install, recognize, remove and retry make none while it is
  off.

**What ships in the manifest.** The libraries ML Kit brings declare components that would run in the process whether
or not the person ever turns the recognizer on. The app's manifest and build remove what ML Kit does not need to
download a model and read strokes, and the merged manifest of the debug build was read to check it:

- Removed: `com.google.mlkit.common.internal.MlKitInitProvider`, the content provider that starts ML Kit when the
  process starts (`tools:node="remove"` in `AndroidManifest.xml`; the app calls `MlKit.initialize` itself, see above).
  WorkManager, which the recognizer declares as a dependency and nothing of ML Kit calls (`exclude(group =
  "androidx.work")`): its entry in `androidx.startup.InitializationProvider` (`WorkManagerInitializer`), its three
  services (`SystemAlarmService`, `SystemJobService`, `SystemForegroundService`), its eight receivers, Room's
  `MultiInstanceInvalidationService`, and the permissions `WAKE_LOCK`, `RECEIVE_BOOT_COMPLETED` and
  `FOREGROUND_SERVICE`.
- Still there, because ML Kit reads or uses them once it runs: the permission `ACCESS_NETWORK_STATE` (the recognizer and
  Datatransport declare it, "for downloading"); `MlKitComponentDiscoveryService`, not exported, which ML Kit reads for
  the registrars of the common library and of digital ink when it is started and which nothing starts; the Datatransport
  declarations `TransportBackendDiscovery` (the backend list, read when the transport is used),
  `JobInfoSchedulerService` and `AlarmManagerSchedulerBroadcastReceiver` (the transport's own retries, with no intent
  filter); `GoogleApiActivity` and the `com.google.android.gms.version` entry of Google Play services' base library.
  Android starts none of these by itself: they act only after ML Kit has run and the transport has something to send,
  which is after the person turned the recognizer on.
- `androidx.startup.InitializationProvider` stays for the app's own libraries (`EmojiCompatInitializer`,
  `ProcessLifecycleInitializer`, `ProfileInstallerInitializer`), none of them ML Kit's.

**The license of ML Kit, and what the gate does with it.** The repository is Apache-2.0, and the gate allows only
permissive licenses (ADR 0149). The libraries ML Kit brings are not open source:

| Library | License |
| --- | --- |
| `com.google.mlkit:digital-ink-recognition` 18.1.0, `com.google.mlkit:common` 18.6.0 | ML Kit Terms of Service (Google's, proprietary; free to use in an app; no source) |
| `com.google.android.gms:play-services-base` 18.1.0, `-basement` 18.1.0, `-tasks` 18.0.2 | Android Software Development Kit License (Google's, proprietary) |
| Firebase components and encoders, Datatransport, OkHttp, Okio | Apache-2.0 |

The recognizer also declares WorkManager; neither it nor ML Kit's common library calls it, and the app excludes it
(`exclude(group = "androidx.work")`), which also removes Room and the manifest entries below.

None is copyleft, and no GPL code is linked: `third-party-notices:check` still fails for one. The
proprietary ones are listed in `scripts/third_party/gradle.json` and in `exceptions` of `scripts/third_party/policy.json`,
each with its reason. The gradle part of `third_party_sources.py` honours `exceptions` as the cargo part does.

**The owner accepted ML Kit on 2026-11-08.** It is free of charge. Each of its exceptions carries
`"accepted": {"by": "owner", "date": "2026-11-08"}`; `release_checks.exception_problems` lets an accepted exception
pass `release_flow.py collect` and `publish` and still stops at one nobody accepted, and a copyleft entry (GPL, AGPL, LGPL,
MPL, EPL, CDDL, EUPL, SSPL, CC-BY-SA) cannot be accepted at all, `by` must be on an explicit list (`ACCEPTERS`: only
"owner"), and `date` must be a real date; `load_policy` refuses anything else. There is no blanket bypass: an acceptance
names one package and one license (each of the five libraries above has its own entry), and a new exception is refused
by a release until the owner accepts it the same way. A test loads the real policy file and shows that an exception
without `accepted` fails a release. The `LicenseRef-*` files under `scripts/third_party/spdx/` are pointers to the
terms' URLs, not the terms' text (each file says so), and the release notes say that the APK contains proprietary runtime libraries (`docs/release.md`).
The transitive entries (`"transitive": true`) are written by hand and are not compared with the resolved Gradle
graph. Follow-up: a gate step that runs `gradle :app:dependencies` and compares the Google coordinates and versions
with `gradle.json`.

**What the person is told.** The ML Kit terms require the developer to inform users about Google's processing of ML
Kit metrics data (<https://developers.google.com/ml-kit/terms>: "You are responsible for informing users of your app
about Google's processing of ML Kit metrics data", and "The ML Kit APIs also send metrics about the performance and
utilization of the APIs in your app to Google"). Google's data disclosure
(<https://developers.google.com/ml-kit/android-data-disclosure>, read on 2026-11-09, which lists digital ink recognition
under "Entity extraction and digital ink recognition") names what ML Kit collects: device information (manufacturer,
model, OS version and build) and the available ML hardware accelerators; the package name and app version;
per-installation identifiers that are not intended to uniquely identify a user or a physical device; performance
metrics such as latency; the API configuration; the input and output size; the feature version; events (feature
initializations, model downloads, detection, resource releases) and the error codes of those events; and, for digital
ink recognition, the configured languages. Google adds that it encrypts these data in transit (HTTPS) and does not
transfer them to third parties. The disclosure does not name the identifiers or say how they are reset, and it does not
list the content of the input. So the text says: the content of what you write is not in the data Google lists (it does
not assert that nothing of it is ever sent, because that is Google's to say), and Soyeht sends none of it. Downloading
the model contacts Google's servers, which see the device's IP address, as the Hugging Face download does for the
dictation models (`PRIVACY.md`). `PRIVACY.md` (the exception to "no analytics library", the list of what leaves the
device, "Handwriting on Android"), the Settings row and this ADR carry the same list; a Google Play data-safety form
declares it. Datatransport is the SDK's transport for these metrics; the app sends neither the strokes nor the text.

## Alternatives

- **Gboard's handwriting layout (the stopgap).** Works today, is the keyboard's own, and needs a switch of layout for
  each use; this change does not touch it.
- **Rasterize the strokes and read them with an open OCR engine** (Tesseract, Apache-2.0). Permissive, but OCR reads
  printed text, not handwriting, and would be poor for the pen.
- **No recognizer below Android 14.** Keeps the repository permissive, and leaves the owner's device without the feature.

## Consequences

- A new dependency with a proprietary license, accepted by the owner: the review is required; a release passes it because of the recorded acceptance, and `PRIVACY.md`, the Settings row and this ADR carry the disclosure.
- The APK grows by the ML Kit libraries; the model is not in it.
- On Android 14 and later the code is not used; the platform's handwriting takes over.
