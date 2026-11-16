# Contract 20 · The voice loop

Status: v1, slice 7 of ADR 0166 (the Apple clients; the Windows and Linux app implements the same rules in
slice 8a). Code: `VoiceTurnMachine`, `SpeechText`, `SentenceChunker`, `HeardSpeech` and
`VoiceLoopController` in `apps/shared`; the two optional models in `apps/dictation`. Vectors:
`contracts/vectors/voice-turns.json` and `contracts/vectors/speech-text.json`.

The loop runs in the client that has the microphone and speaker. Speech is recognised and spoken on that
device; only text crosses the wire, as the person's next prompt and the agent's reply. This contract adds
no method, event or capability: it is the rules two clients must share.

## 1. What is shared and what is not

Shared, as vector files that every client passes: the turn machine, the speech text and the sentence
chunker, and the part of a reply the person heard. They are pure and deterministic: time is data, never a
clock.

Not shared, and not rules: the audio path (the operating system's echo cancellation and device handling),
the speech and turn models behind their seams, the voices, and what the agent says. The persona is text the
agent follows. Nothing here encodes how long the avatar speaks, in what style, or whether it acknowledges
the person while it thinks.

## 2. The turn machine (`voice-turns.json`)

Four states, in eight phases:

| State | Phases | Meaning |
| --- | --- | --- |
| `idle` | `off` | the loop is not running |
| `listening` | `waiting`, `hearing`, `deciding` | nobody talks; the person talks or paused briefly; the pause is long enough that the end-of-turn model was asked |
| `thinking` | `finalizing`, `waitingReply` | the turn ended and the recognizer is producing its final text; the words went to the agent and nothing is audible yet |
| `speaking` | `playing`, `probing` | the reply is audible; someone talks over it and the machine listens to what they say |

**Inputs**, each with the time it happens in milliseconds on the microphone's clock: `start`, `stop`,
`frame` (the speech probability of one 32 ms frame, stamped with the time its audio ended), `endOfTurn`
(the model's answer to a check by its `id`), `transcript` (partial or final), `sentenceStarted`,
`replyFinished`, `interrupt` (a key or click). Frames arrive every `frameMs` while the microphone is on, and
they are the only clock: the machine has no timers.

**Actions** it asks for: `startTurn` and `probeInterruption` (start a recognizer with the audio from
`fromMs` on), `checkEndOfTurn`, `finalizeTurn`, `discardTurn`, `dropProbe`, `submit` (the words, and whether
the person interrupted a reply), `cancelReply`, `stopPlayback`.

**Rules**, with the numbers of `defaults` in the file:

- A frame is voiced at `startThreshold` (0.5) or more, or at `endThreshold` (0.35) or more when the frame
  before it was voiced.
- The person has started to talk after `speechStartMs` (96) of unbroken voiced frames. The recognizer
  starts with the audio from `preRollMs` (320) before the first voiced frame, never before 0.
- After `silenceMs` (350) of silence the end-of-turn model is asked, once per pause. Speech that resumes
  first makes its answer stale. A "finished" answer ends the turn; a "not finished" answer waits, and the
  turn still ends after `incompleteSilenceMs` (2000) of silence. With no model the answer is always
  "finished".
- The turn ends with the recognizer's final text. Blank text discards the turn. Otherwise the text goes out
  with `submit`, and the machine waits for the reply.
- Speech for `speechStartMs` while the agent is still thinking cancels the reply and starts a new turn.
- Talking over a reply: after `bargeInMs` (350) of unbroken voiced frames, a recognizer listens. The talk
  takes the turn (the reply is stopped and cancelled, and the words go out with `interrupted`) unless the
  text is a **listener's noise** (an entry of `continuers`, such as "uh-huh", or the start of one) or an
  **echo** (the words of the last two sentences played, the last word possibly unfinished). Words are
  compared as lowercase runs of letters, combining marks and digits (Unicode categories L, M and N), anything
  else being a break. A probe that decides nothing is dropped after
  `probeTimeoutMs` (1500) or `silenceMs` of silence, and the next one needs a pause first.
- `halfDuplex` (the Windows and Linux launch mode): the microphone is not heard while the avatar speaks;
  only `interrupt` stops it, and speech that was heard meanwhile does not count once the reply ends.
- `interrupt` and `stop` cancel what is under way in the phase they find the machine in.

The vector file holds the defaults and the cases. A case is a list of steps; a `frames` step is `count`
frames of probability `p`, the first ending at `at`, one every `frameMs`. Each step lists the actions the
machine returned, each with the time it happened, and the phase it ended in.

## 3. Speech text, sentences and the heard part (`speech-text.json`)

**Speech text** is what of a reply is read aloud, from the Markdown subset every client draws (ADR 0087):
one line per paragraph, heading or list item, joined by newlines. Code blocks and tables are not read, nor
struck-out text or a rule. A link is read as its label and a bare web address is dropped. Emoji and
pictographs (U+1F000 to U+1FAFF, U+2600 to U+27BF, U+2B00 to U+2BFF, and the joiner and variation selector)
are dropped, and a line with no letter or digit left is dropped. A task-list marker is dropped and blanks
are collapsed. A reply still arriving is parsed with the streaming close of ADR 0087.

**Sentence chunker.** It takes the speech text so far (the text may only grow) and says which sentences
became whole. Lengths count Unicode scalar values and a blank is a character with the Unicode `White_Space`
property; `chunkerDefaults` holds the two limits.

- A sentence ends at `.`, `!`, `?` or `…` (and any closing quote or bracket after them) when a blank
  follows, or at the end of a finished line; at `。`, `！` and `？` at once. A full stop in a number or after
  an abbreviation is not an end.
- A line ends at a newline, and sentences of two lines are never joined.
- A sentence under `minimumLength` waits for the next one of its line, so a voice is not asked for "Yes."
  alone; the last sentence of a finished line is spoken as it is.
- A sentence over `maximumLength` is cut at the last `,`, `;` or `:` before the limit, else at the last
  blank, else at the limit. An unfinished tail that outgrows the limit is spoken in parts.
- When the reply is final, what is left is spoken as it is.

**Heard.** What the person heard of the reply they interrupted: the sentences played to their end, and
the whole words of the one playing that the share of its audio already played covers (the share is clamped
to 0 to 1, and a word is heard only when all of it is).

## 4. The seams

`VoiceActivityDetecting` (the speech probability of a 512-sample frame at 16 kHz) and `EndOfTurnDetecting`
(whether the last 8 seconds of the turn are a finished turn) are the two model seams, listed in the
catalogues of `apps/dictation` (`VoiceModelCatalogue`) like a dictation engine or a voice: a descriptor with
its license and `commercialUse`, an install that is the person's to start, and a fallback that needs no
model (loudness; the pause alone).

| Model | Runs as | License |
| --- | --- | --- |
| Silero VAD | CoreML, through the `SpeechVAD` product of the speech package that already supplies the other local models | MIT |
| Smart Turn v3.2 | CoreML (`aufklarer/Smart-Turn-v3.2-CoreML`), read directly | BSD-2-Clause |

The audio is `VoiceAudio`: one engine that records through the system's voice processing (echo
cancellation) and plays the avatar's speech, so a person can talk over the avatar through the loudspeaker.
While it runs, every recognizer's microphone joins it instead of opening the device.

The stage reads, from `VoiceLoopController`: `state`, `phase`, `inputLevel`, `outputLevel` (the loudness of
what plays, about every 16 ms), and `currentSpeech` with `playbackPosition` (the audio of the sentence being
heard, with the voice's own word or phoneme timings when it has them).
