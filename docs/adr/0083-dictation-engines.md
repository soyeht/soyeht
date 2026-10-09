# ADR 0083 · Dictation engines are a catalogue

Date: 2026-10-08
Status: accepted.

## Context

The composer's mic (PR #456) listens through Apple's Speech framework only. The owner
wants to choose the engine behind it, starting with NVIDIA Nemotron 3.5 ASR Streaming,
and wants adding the next engine to be easy. The choice must work in the composer on the
Mac, the iPad and the iPhone, and must live in Preferences.

## Decision

### One interface, one descriptor, one catalogue

- `SpeechRecognizing` (`apps/shared`) is the only interface an engine implements:
  `authorize`, `start(options, onEvent)`, `finish`, `cancel`. `SpeechSessionOptions`
  carries the language tag and the vocabulary.
- `DictationEngine` is the descriptor of an engine: id, display name, badge, summary,
  platforms, languages, whether it uses the vocabulary, its model (size, licence,
  installer, `isInstalled`, `remove`) when it needs an install, a runtime availability
  check, and a factory. "Requires install" is data: an engine requires an install exactly
  when it has a model (`requiresInstall`).
- `DictationEngineRegistry` is the catalogue. Its first engine is the fallback: Apple's,
  which needs no download and runs everywhere the composer does.
- `DictationCatalogue` (`apps/dictation`) is the app's list of engines. It is the only
  place an engine is listed.

Preferences, the badge above the mic, the shortcut and the composer are generated from the
registry through `DictationEngineStore`. None of them knows an engine by name.

### Shared plumbing

An engine reuses these and writes none of them:

| Concern | Type |
| --- | --- |
| Microphone permission, audio engine, realtime tap | `MicrophoneCapture` |
| Conversion to the model's mono float samples | `AudioResampler` |
| System callbacks that arrive on a background queue | `SystemCallback.value` |
| Install, progress, retry, remove, one install at a time | `DictationModelStore` |
| The person's vocabulary | `DictationVocabularyStore` |
| Choice of engine and language, fallback, shortcut | `DictationEngineStore` |

The tap block runs on the realtime audio thread and every system callback on a queue of
its own. They are built with `@Sendable` closures and never inherit the main actor (the
cause of the crash fixed in #465).

### Behavior

- **Apple is the default engine.** An engine that requires an install is opt-in: nothing
  downloads until the person presses Install for it in Preferences > Dictation. Choosing
  it, using the mic and pressing the shortcut never start a download, and an engine that is
  not installed cannot be selected.
- The install step shows the engine's name, the download size and the licence, then an
  Install button with progress. When the install finishes the engine is selected. A failed
  install says why and offers Retry; nothing retries by itself.
- At launch the app loads the models that are already installed (from disk, with no
  network). While one loads, or if it fails to load, Apple's engine keeps dictating and the
  mic says why.
- Remove download deletes the model's files and drops it from memory. The engine is
  deselected and Apple's takes over. It is not offered while an install runs.
- An engine that cannot run on the device is listed with the reason and cannot be picked.
- The language list follows the selected engine. A language the engine does not ship falls
  back to System default. System default becomes a tag only when the engine ships exactly
  that language: a device set to English in Brazil reports `en_BR`, and guessing `en`
  would transcribe Portuguese speech as English.
- The vocabulary (up to 200 phrases) is sent only to engines that declare they use it.
- The badge above the mic names the engine that listens, while it listens and for two
  seconds after the shortcut switches engine.
- Shortcut: Control-Command-E, from the Mac menu and from a hardware keyboard on the iPad.
  Command-Shift-E is the editor's project panel. It cycles only the installed engines. When
  Apple is the only one installed and another engine needs an install, it opens
  Preferences > Dictation (Settings on the phone) at that engine's install step instead of
  downloading.

### Nemotron

`NemotronEngine` runs the INT8 CoreML bundle of Nemotron 3.5 ASR Streaming (600M
parameters, cache-aware FastConformer encoder with an RNN-T decoder) through
`speech-swift`'s `NemotronStreamingASR`.

| | |
| --- | --- |
| Model | `aufklarer/Nemotron-3.5-ASR-Streaming-0.6B-CoreML-INT8`, a conversion of `nvidia/nemotron-3.5-asr-streaming-0.6b` |
| Download | about 642 MB (encoder 592 MB, decoder 30 MB, joint 19 MB, tokenizer and vocabulary 0.6 MB), fetched once at runtime into the downloader's cache when the person presses Install. No weights are committed or bundled. A marker file written after a complete download and load is what "installed" means. |
| Licence of the model | OpenMDW 1.1, the same as the NVIDIA model (https://openmdw.ai/license/1-1/) |
| Licence of the runtime | `speech-swift`, Apache-2.0, pinned to one revision in `apps/dictation/Package.swift` |
| Languages | 19 transcription-ready locales plus automatic detection |
| Vocabulary | Biases decoding toward the phrases; the model suggests how hard to push each one |

Platforms:

| Device | Rule |
| --- | --- |
| Mac | macOS 15, Apple silicon |
| iPad | iOS 18, arm64, at least 6 GB of memory |
| iPhone | iOS 18, arm64, at least 6 GB of memory |

`speech-swift` declares macOS 15 and iOS 18 and ships CoreML models, which run on iOS. It
publishes no iPhone or iPad measurements, so the memory floor is our own: the compiled
models take more than a gigabyte. The floor has not been measured on a physical iPhone or
iPad. If loading fails there, the download state shows the error and Apple's engine keeps
dictating.

### Deployment floor

`speech-swift` requires macOS 15 and iOS 18, and Xcode refuses to link a package that
requires more than the app targets. The Mac and iPhone/iPad apps therefore move from
macOS 14 and iOS 17 to macOS 15 and iOS 18. `apps/shared` keeps its lower platforms.

### Dependencies stay out of `apps/shared`

`speech-swift` resolves about 35 packages (MLX, WhisperKit, an HTTP server stack and
others) although the app builds only the Nemotron target and MLX. To keep
`swift test --package-path apps/shared` light, the engine lives in its own package,
`apps/dictation` (`SoyehtDictation`), which depends on `apps/shared`. The apps link
`SoyehtDictation` and install `DictationCatalogue.registry` at launch. A future engine with
no third-party dependency can live in `apps/dictation` too.

Considered and rejected: porting the model's mel front end, RNN-T decoder and word
boosting to Swift ourselves (large, and it would have to track upstream fixes).

## How to add an STT engine

1. Add one file to `apps/dictation/Sources/SoyehtDictation/` with the engine: a
   `SpeechRecognizing` implementation and a `DictationEngine` descriptor (id, names,
   platforms, languages, `usesVocabulary`, `model` if it downloads, `availability`,
   `make`). Use `MicrophoneCapture` for the audio and `AudioResampler` for the samples. An
   engine with a model gives the descriptor an installer that reports `DictationModelStage`
   and loads from disk without network when the model is installed, plus `isInstalled` and
   `remove`; do not write a permission request, an install state or a vocabulary store.
2. Add the descriptor to `DictationCatalogue.registry`.
3. Add tests with a fake engine, as in `DictationEngineTests`.

Nothing else changes: not the composer, not Preferences, not `DictationController`, not
the menu. `aThirdEngineAppearsInPreferencesAndIsUsedWithNoOtherChange` registers a fake
third engine and checks that it is listed, selectable, reachable by the shortcut and used by
the mic.

## Consequences

- Dictation no longer depends on one vendor, and the fallback keeps it working while a model
  downloads.
- Apps no longer run on macOS 14 or iOS 17.
- Building the apps compiles MLX the first time, and the lock file of `apps/dictation`
  records the third-party packages.
- The 642 MB download starts only when the person presses Install, and they can remove it
  again.
