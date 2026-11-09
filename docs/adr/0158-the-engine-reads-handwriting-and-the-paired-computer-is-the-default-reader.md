# ADR 0158: The engine reads handwriting, and the computer a device is paired with is the default reader

- Status: accepted. Extends ADR 0079 (nothing slow under the Core lock) and ADR 0086 (the system seams). Beside ADR 0157, which is the device's own recognition and stays an opt-in setting.
- Date: 2026-11-09

## Context

A person who writes with a pen on the phone or the tablet wants text in the message field. The
Android app (ADR 0157) can read the strokes on the device with a library of the platform's vendor,
but that is a model that is downloaded, a license that is not open source, and a recognizer of the
vendor's choice. The computer the device is paired with is already trusted with the whole session
(ADR 0132), already runs the engine, and has a recognizer built into the system on the Mac. The owner
decided on 2026-11-08 that recognition runs on the paired computer by default, with the device's own
recognizer as an opt-in setting, and that a recognizer need not be open source but must send no data.

A measurement on real pen strokes (UJIpenchars2, composed into Portuguese and English words and
phrases, drawn as round-capped lines and read by `VNRecognizeTextRequest` at the accurate level with
`pt-BR` and `en-US`) read about 70% of the words exactly, with a median of 170 ms a call on this Mac.
A smaller picture read better than a larger one: a small letter 48 px high beat 96 px.

## Decision

**The engine reads, and the method is part of the contract.** `ink.recognize` takes the strokes of the
pen and the languages the person writes, and answers candidates, the recognizer's name and the
language. It is not a mutation. `hello` lists the capability `ink-recognition` when the engine has a
recognizer, so a client offers the pen without a failed call. The contract is
`docs/contracts/14-handwriting-recognition.md`, the numbers are `x-limits` of `protocol.json`
(`ink*`: 64 strokes, 2000 points a stroke, 8000 points, 10 candidates, below `jsonBytes`), and the
four errors are `ink_unsupported` (with `data.detail` as `InkUnsupportedReason`: `no_os_support`,
`language_pack_missing`, `model_not_installed`), `ink_language_unavailable`, `ink_limit_exceeded` and
`ink_recognition_failed`. Time stays in the request (`t`) so that a recognizer that reads the order
and speed of the pen needs no new method.

**An image pipeline, with one backend per system behind one shape.** The strokes are drawn as an
image and a recognizer reads the image. Everything but the last step is the same code on every system:
`engine/soyeht-ink` (layer 3, depending only on `soyeht-protocol` and, for the rule against the Core
lock, `soyeht-process`) checks the limits, estimates the height of a small letter from the writing
and draws it at 48 px with a pen about 4 px wide (anti-aliased capsules, no dependency), matches the
languages, puts the pieces of several lines in reading order and tidies the candidates. The last step
is the trait `Recognizer` and `soyeht_ink::system()`, which has the same public shape everywhere: a
`cfg` module of the crate, as the rule on system seams allows a crate to keep one for a concern of
its own. On macOS it is Apple Vision. On Linux and Windows it is the typed refusal
`ink_unsupported`/`no_os_support`; a recognizer for them is a later backend that changes no contract.

**Vision is called in process through the `objc2` crates**, not through a helper program.
`objc2-vision`, `objc2-core-graphics`, `objc2-core-foundation` and `objc2-foundation` (MIT or
Apache-2.0, already the family `soyeht-engine` uses) give the calls with a small `unsafe` surface,
each block with its reason. A helper program would add a binary to sign and package, a process to
start and stop, a protocol between it and the engine and a place to leave strokes on a pipe, for a
call that takes milliseconds. Vision sends nothing off the machine, needs no permission, no
entitlement and no download. An
Objective-C object lives only inside one call, in an autorelease pool.

**Off the lock, one at a time.** The request is checked under the Core lock (a bounded walk over at
most 8000 points) so an oversize request is answered at once. Drawing and recognizing run through
`Outcome::Offload` with a deadline of 10 s, `concurrent` so the connection goes on, behind a
`SharedPort` so one recognition runs at a time. `soyeht_ink::recognize` itself refuses to run on a
thread that holds the Core lock (`soyeht_process::forbidden`), the rule ADR 0079 states for commands.
A test engine with a slow fixture recognizer shows that another request is answered during a
recognition and that the recognizer ran with no lock.

**The engine keeps and says nothing about the writing.** The strokes, the image and the text live
for the length of one request. They are not stored, not put in an event, not logged, and an error
holds no text (`soyeht_ink::Error` has a message per kind and a static step name, never data). The
engine writes no log line for a recognition, so there is no log of it to rotate or to leak.

**The tests that need Vision run where it is.** The crate has unit tests of the limits, the
rasterizer, the language matching and the candidates on every system, and tests that draw block
capitals and have Vision read them. They run on a Mac and print that they were skipped where
`system()` answers `no_os_support`; the production code holds no switch for them. The engine's
integration test installs a recognizer through the `Hooks` seam of ADR 0091 (`SOYEHT_TEST_INK_RECOGNIZER_MS`,
read only by `test_hooks`), which no product build contains.

## Consequences

- A paired device with a pen gets text from the Mac the same way it gets a terminal, with no model
  on the device. The device's own recognizer (ADR 0157) is a setting for a person who prefers it or
  who is away from the computer; both answer the same shape of candidates to the same field.
- About 70% of the words are exact, and the candidates are for the client to offer, not to type on
  its own. A better scale estimator, a second pass at another size or a stroke-based recognizer
  change `soyeht-ink` only.
- The first Linux or Windows backend needs a recognizer that sends no data; until then those systems
  answer a typed refusal and the capability is absent.
- The strokes are drawn without a client's help, so a client sends them as they are. A pen that
  draws lines of different weight, or a stylus pressure, is not in the contract.
- One more crate (`soyeht-ink`) and the `objc2-vision` family are dependencies on the Mac; their
  licenses are permissive (ADR 0149).
