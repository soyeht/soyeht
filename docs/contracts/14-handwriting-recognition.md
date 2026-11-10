# Contract 14 · Handwriting recognition

Status: v1. Decision: ADR 0158. Systems: macOS (Apple Vision); Linux and Windows answer
`ink_unsupported` until they have a recognizer. Code: `engine/soyeht-ink`, dispatched by
`engine/soyeht-engine/src/ink.rs`. Clients: any that has a pen (the Android app first).

A person who writes with a pen on a phone or a tablet gets text, read by the computer the device
is paired with. The client sends the strokes; the engine answers with candidates. Recognition on
the device itself is the client's own opt-in setting (ADR 0157) and is not this contract.

## 1. Whether the engine can read

`hello` lists the capability `ink-recognition` in `capabilities` when the engine has a recognizer
on its system, and the state snapshot's `capabilities` section lists it too. A client offers the
pen only when it is listed, and never has to fail a call to find out. The list is the engine's
choice at its start: it says that a recognizer exists, not that every language is installed
(section 5).

## 2. The method

`ink.recognize {strokes, languages, maxCandidates?}` → `{candidates, recognizer, language}`.
Not a mutation: it changes nothing, so it takes no `idempotencyKey`, and a repeated call runs
again. A human connection may call it, local or a paired device; a pane's agent may not.

**Request.**

- `strokes` is the writing, in the order it was drawn: a list of strokes, each a list of points
  `{x, y, t}`. `x` and `y` are numbers in the client's own units with `y` growing downward, as on
  a screen; the engine scales the writing, so the unit does not matter. `t` is the milliseconds
  since the first point of the writing, an integer, not decreasing in a well-formed request. The
  Apple Vision recognizer reads an image and does not use `t`; it is part of the contract so a
  recognizer that reads the order and speed of the pen needs no new method.
- `languages` is the list of BCP 47 tags the person writes in, most likely first (`pt-BR`,
  `en-US`). At least one.
- `maxCandidates` is the most candidates wanted, 1 to `inkCandidates`; `inkCandidatesDefault`
  when absent.

**Result.**

- `candidates` is the text read, the likeliest first, each `{text, score?}`. `score` is 0 to 1
  when the recognizer says how sure it is, and absent when it does not. Candidates are trimmed,
  not empty and distinct, no longer than `inkCandidateBytes`, and no more than asked. Writing
  that cannot be read at all is an answer with no candidates, not an error. Several lines are
  one candidate, the lines joined by a line feed in reading order.
- `recognizer` names what read it (`apple-vision`). It is a label for a person or a log, not a
  value a client branches on.
- `language` is the first of the requested languages that the recognizer reads, as the
  recognizer names the tag (`pt-PT` asked, `pt-BR` answered). Vision does not say which of
  several languages it matched, so this is the language the reading was asked to favour first.

## 3. Limits

| Limit | Value | Applies to |
| --- | --- | --- |
| `inkStrokes` | 64 | strokes in a request |
| `inkStrokePoints` | 2000 | points in one stroke |
| `inkTotalPoints` | 8000 | points in all the strokes |
| `inkCandidates` | 10 | `maxCandidates` and the candidates answered |
| `inkLanguages` | 8 | tags in `languages` |
| `inkLanguageBytes` | 35 | bytes in one tag |
| `inkCoordinate` | 1000000 | the largest `x` or `y`, in absolute value |
| `inkDurationMs` | 3600000 | the largest `t` |
| `inkCandidateBytes` | 2048 | bytes of one answered candidate (longer is cut at a character) |

8000 points as JSON are about 350 kB, under `jsonBytes`. The engine checks every limit before it
draws or recognizes anything, and a request over one is answered at once.

## 4. Errors

| Kind | When |
| --- | --- |
| `ink_limit_exceeded` | A request is over a limit of section 3. The message names which. |
| `invalid_params` | The request is malformed: no strokes, a stroke with no points, no language, a tag that is not a tag, a coordinate that is not a finite number, `maxCandidates` 0. |
| `ink_language_unavailable` | The recognizer reads none of the languages asked. The client may offer other languages or try again with another list. |
| `ink_unsupported` | The engine cannot read handwriting at all, and `data.detail` says why: `no_os_support` (the system has no recognizer), `language_pack_missing` (the system has one but the language data is not installed), `model_not_installed` (it needs a model nobody installed). A client does not retry and says so to the person. |
| `ink_recognition_failed` | The recognizer failed on a request it could have read. A client may retry once. |
| `internal_error` | A request outlived its deadline (`inkDeadlineSec`), or the engine broke its own rule. |

No message holds the writing or the text.

## 5. How the engine reads

1. **Under the Core lock**, the request is checked against section 3. Nothing else is done there.
2. **Off the lock** (`Outcome::Offload`, ADR 0079), the languages are matched with what the
   recognizer reads, the strokes are drawn as an image and the recognizer reads the image. At
   most one recognition runs at a time; the others wait behind it, inside their deadline, and one
   whose deadline has passed when its turn comes is answered without being read. A
   recognition does not hold back the other requests of its connection (`concurrent`).
3. The answer is tidied (section 2) and sent.

**The image.** Dark, round-capped lines on white, 8-bit gray, the same code on every system. The
scale comes from the writing: the height of a small letter is estimated from the strokes that are
about as tall as the tallest, and the writing is drawn so that it is 48 px high, with a pen
about 4 px wide and a white margin. A writing too long for 3000 px on its longest side is drawn
smaller. The size was chosen by measurement (ADR 0158).

**Languages.** A requested tag matches a recognizer's tag by the whole tag, ignoring case, or
else by its primary language. The matched tags, in the order asked, are what the recognizer is
told to read.

**Apple Vision.** `VNRecognizeTextRequest` at the accurate level with language correction on.
One observation gives the candidates Vision lists for it, with their confidence; several
observations are put in reading order, top to bottom and left to right, and read as one
candidate scored by the mean of theirs. It sends nothing off the machine and needs no
permission and no download.

## 6. What the engine keeps

Nothing. The strokes, the image and the text exist for the length of one request, in memory.
They are not written to the database, to a file or to the log, and no event is published for a
recognition. A log line about recognition, if one is ever written, holds a kind and never the
text (`soyeht_platform::log`). The cached list of languages the system reads holds no ink.

## 7. Clients

The Swift `SoyehtClient` wraps the method as `EngineClient.recognizeInk`; it needs no state. The
Rust clients call the generated `method::INK_RECOGNIZE` with the generated types. A client sends
its strokes as they are, in its own units, and does not scale, smooth or resample them.
