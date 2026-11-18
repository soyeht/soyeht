# Contract 20 · The voice loop

Status: v1, slice 7 of ADR 0166 (the Apple clients; the Windows and Linux app implements the same rules in
slice 8a). The slice lands in three changes: this one holds the specification (sections 1 to 3: the policy, the
vectors and the pure rules, `VoiceTurnMachine`, `SpeechText`, `SentenceChunker`, `ReplyFeed` and `HeardSpeech` in
`apps/shared`); the second adds the two optional models (`VoiceModelCatalogue` in `apps/dictation`); the third
adds the audio session and `VoiceLoopController`. Section 4 says what those two must do, and lands as code with
them. Vectors: `contracts/vectors/voice-turns.json` and `contracts/vectors/speech-text.json`, written by
`contracts/voice_vectors.py`. Numbers: the `voice` group of `x-clientPolicy` in `contracts/protocol.json`
(`ClientPolicy.voice...` in Swift, `client_policy::VOICE_...` in Rust).

The Rust implementation of sections 1 to 3 is the `soyeht-voice` crate of the desktop workspace
(`apps/desktop/crates/soyeht-voice`, slice 8a of ADR 0166): it passes both vector files, and it sits on the
client's Markdown parser because the speech text is read from it. The audio, the models and the controller
of the Windows and Linux app are not in it.

The loop runs in the client that has the microphone and speaker. Speech is recognised and spoken on that
device; only text crosses the wire, as the person's next prompt and the agent's reply. This contract adds
no method, event or capability: it is the rules two clients must share.

## 1. What is shared and what is not

Shared, as vector files that every client passes: the turn machine, the speech text, the sentence
chunker, the reply feed, and the part of a reply the person heard. They are pure and deterministic: time is
data, never a clock. Their numbers (frame size, timings, listener noises, sentence limits, stops, closers and
abbreviations) are not repeated in the vectors: they are `x-clientPolicy.voice`, and the cases are written
for those values. A case that needs another value says so in its `config`. Changing a value of the policy
changes cases of the vectors in the same change.

Not shared, and not rules: the audio path (the operating system's echo cancellation and device handling),
the speech and turn models behind their seams, the voices, and what the agent says. The persona is text the
agent follows. Nothing here encodes how long the avatar speaks, in what style, or whether it acknowledges
the person while it thinks.

## 2. The turn machine (`voice-turns.json`)

Four states, in nine phases:

| State | Phases | Meaning |
| --- | --- | --- |
| `idle` | `off` | the loop is not running |
| `listening` | `waiting`, `hearing`, `deciding` | nobody talks; the person talks or paused briefly; the pause is long enough that the end-of-turn model was asked |
| `thinking` | `finalizing`, `waitingReply`, `probingWait` | the turn ended and the recognizer is producing its final text; the words went to the agent and nothing is audible yet; someone talks meanwhile and the machine listens to what they say |
| `speaking` | `playing`, `probing` | the reply is audible; someone talks over it and the machine listens to what they say |

**Inputs**, each with the time it happens in milliseconds on the microphone's clock: `start`, `stop`,
`frame` (the speech probability of one frame, stamped with the time its audio ended), `endOfTurn` (the
model's answer to a check by its `id`), `transcript` (partial or final), `sentenceStarted`, `replyFinished`,
`interrupt` (a key or click). Frames arrive every `frameMs` while the microphone is on, and they are the only
clock: the machine has no timers. A frame that does not arrive stops the machine; the client watches for
that (section 4).

**Actions** it asks for: `startTurn` and `probeInterruption` (start a recognizer with the audio from
`fromMs` on), `checkEndOfTurn`, `finalizeTurn` and `finishProbe` (stop feeding the recognizer and wait for
its final text), `discardTurn` and `dropProbe` (drop the recognizer and what it heard), `submit` (the words,
and whether the person interrupted a reply), `cancelReply`, `stopPlayback`.

**Rules**:

- A frame is voiced at `startThreshold` or more, or at `endThreshold` or more when the frame before it was
  voiced. A voiced run of `speechStartMs` or more is **speech**; a shorter run is a blip, which neither starts
  a turn nor breaks a silence. Silence is counted from the last frame of speech.
- The person has started to talk when speech begins in `waiting`. The recognizer starts with the audio from
  `preRollMs` before the first voiced frame, and never from before `historyMs` ago (what the client still
  keeps), nor before 0.
- After `silenceMs` of silence the end-of-turn model is asked, once per pause. Speech that resumes first makes
  its answer stale (a check number is never reused, not even after a stop). A "finished" answer ends the
  turn. A "not finished" answer, or no answer at all, waits, and the turn still ends after
  `incompleteSilenceMs` of silence. With no model the answer is always "finished".
- Ending the turn asks the recognizer for its final text and waits for it (`finalizing`), at most
  `finalizeTimeoutMs`; then the last partial text is the final one (the recognizer is dropped, and a blank text
  discards the turn). The last partial text is the last one the turn's recognizer gave, whether it came while
  the person talked, during the pause or while the machine waited, and a probe that took the turn gave its
  partial text to the turn. A turn keeps nothing of the one before: a turn whose recognizer heard nothing is
  blank, whatever the turn before heard. **A recognizer that ends before the turn does** (it failed, or it gave its final text
  early) has given its text. If the person is still talking when it ends, a recognizer starts at once for the
  rest; if they talk again after a pause, one starts then; and the words of the recognizers are one turn. A
  recognizer that starts after another ended in the same turn never starts from audio before the point where
  the first one ended; a recognizer of a new turn, or of a probe, starts from `preRollMs` before the voice,
  whatever the turn before did. If the person does not talk again, the turn ends with that text at once, with no wait. A recognizer that
  heard nothing of the new speech leaves the words of the first as they were: they are not said twice.
- Blank text discards the turn. Otherwise the text goes out with `submit`, and the machine waits for the reply.
- **Talking over the wait or over the reply** is listened to before it is believed. After `bargeInMs` of
  unbroken voiced frames while the agent thinks (`waitingReply`) or its reply plays (`playing`), a recognizer
  listens (`probingWait`, `probing`). A cough is not worth a probe. At the end of the talk (`silenceMs` of
  silence, and a silence of exactly that long is enough, as it is for a turn) the recognizer is finished, and
  its final text decides; the probe is dropped at `probeTimeoutMs`
  from its start whatever happens. The talk **takes the turn** unless the text is nothing, a **listener's
  noise** or an **echo**. Taking the turn cancels the reply, and stops what plays; the words then go out as the
  next turn (with `interrupted` when a reply was playing). If the person goes on talking after the final text
  of that recognizer (after a pause), a recognizer starts for the new speech, and the words of both are one turn.
  - A listener's noise is a text made only of entries of `continuers`, or the first whole words of one ("mm"
    waits, being the first word of "mm hmm"; "aha" is not the start of "aham" and takes the turn). They are
    non-lexical noises ("mhm", "uh huh", "hmm", "aham", "uhum") and nothing else: "yes", "sim", "ok" are answers, and an
    answer to a spoken question takes the turn.
  - An echo is a text of two words or more that appears, as consecutive words, in the last two sentences
    played taken together (so it may cross their boundary); its last word may be unfinished, and every
    other word is a whole word of the sentences ("it i half" is not an echo of "It is half past three"). **One word** that
    equals a word of them or begins one ("no" against "not", "th" against "three") decides nothing while it is a
    partial text, and takes the turn only as the final text, after
    the pause: the first word of the avatar's own echo does not take the turn, and "stop", "no" or "para" works,
    a little later when the avatar had said it too. A single word the sentences do not hold takes the turn at once.
  - Words are compared in Unicode NFC, lowercase, as runs of letters, combining marks and digits (categories L, M
    and N), anything else (a blank, a stop, an apostrophe) being a break. Each character of a script with no
    blanks (`scriptsWithoutBlanks` of the policy: Han, kana, Thai, Lao, Khmer, Myanmar) is a word of its own, so
    those scripts match mid-sentence.
  - **Known limits**: a number said in words is not the number written ("it is three thirty" is not an echo
    of "It is 3:30 now", so it takes the turn), and two characters of a script with no blanks that the avatar
    just said are taken for an echo.
- A probe that decided nothing returns to `playing` or `waitingReply`; the next one needs a pause in the voice
  first (not a talk that never stopped), over the reply as over the wait. That holds when the probe was dropped
  by its limit and when it was dropped by its final text (a listener's noise or an echo) while the person still
  talks. The hold lasts until a frame is not voiced, and it goes on when the reply starts to play: a voice that
  never stopped does not start a probe at the moment the reply plays.
- A reply that starts (`sentenceStarted`) from `waiting` or `waitingReply` is playing: a reply is spoken whether or
  not a voice turn preceded it. The client holds a reply that is ready while the person talks. `replyFinished`
  returns to `waiting`.
- `halfDuplex` (the Windows and Linux launch mode): the microphone is not heard while the avatar speaks; only
  `interrupt` stops it, and the machine stays deaf for `halfDuplexTailMs` after the avatar's voice ends or an
  `interrupt` stops it while it speaks (`playing` or `probing`), for the tail of the speaker; an `interrupt` in
  another phase leaves no tail. A frame stamped before the end of the tail (`at` less than the time of the end
  of the voice plus `halfDuplexTailMs`) is deaf; a frame stamped exactly at that time is heard. Speech heard
  meanwhile does not count, and a recognizer never starts with audio from inside the deaf tail.
- `interrupt` drops what is under way in the phase it finds the machine in, and cancels the agent's reply or work.
  It also forgets what the turn it drops had kept (the words of its earlier recognizers, and that the turn
  took a reply), so the next turn is a new one.
  `stop` (turning the voice off) stops playback and listening and **does not cancel the agent's work**: the agent
  goes on, and its messages are spoken if the loop is started again. Only words that take the turn, or an explicit
  `interrupt`, cancel.
- A `start` of a loop that runs does nothing. The loop stops itself (the audio failed) with `stop`, not `interrupt`. Inputs that mean nothing in a phase do nothing: that is a rule,
  and the `pairs` of the vector file say it for every phase against every input.

**The vector file** holds `reach` (a name for each phase, and the steps that bring a machine to it), `cases`
and `pairs`. A case is a list of steps, optionally after a `reach`, with `config` overrides: any time of the
policy by its name (`frameMs`, `speechStartMs`, `preRollMs`, `historyMs`, `silenceMs`, `incompleteSilenceMs`,
`finalizeTimeoutMs`, `bargeInMs`, `probeTimeoutMs`, `halfDuplexTailMs`), the two thresholds (`startThreshold`,
`endThreshold`), `halfDuplex` and `continuers`; a name that is not one of them fails. A `frames` step is
`count` frames of probability `p`, the first ending at `at`, one every `frameMs` of the case; every other step
is one input. Each step lists the `actions` the machine returned, each with the time it happened, and the
`phase` it ended in. Steps are in time order. The `pairs` put one input of each kind to a machine in each
phase, so no transition is unpinned. A missing or misspelled key fails a client's test.

## 3. Speech text, sentences, replies and the heard part (`speech-text.json`)

**Speech text** is what of a reply is read aloud, from the Markdown subset every client draws (ADR 0087):
one line per paragraph, heading or list item, joined by newlines.

- The text is read with every line break as one newline: `\r\n` and a lone `\r` become `\n` before anything
  else, so a text of any system reads the same.
- Code blocks and tables are not read, nor struck-out text or a rule. A link is read as its label, never its
  address. A bare web address (a word that starts with an `addressPrefixes` entry in any case, after any
  `addressOpeners` character) is dropped, and the run of stops that ended its sentence stays, joined to the
  word before it: the whole run at the end of the word, of `sentenceStops` and of `sentenceWideStops` ("?!",
  "…", "。"). An address followed by closing brackets takes the stop that comes before them with it (a known
  limit: "(see https://example.com.)" loses its stop). HTML tags (`<` then a letter, `/` or `!`, up to the
  next `>`: a comment is one, and so is "<b and c>") are dropped; a `<` with a blank after it, or a tag never
  closed, is text.
- Emoji and symbols a voice would read out as their names, the code points of `pictographs` (pictographs, arrows,
  hourglass, the keycap mark, the joiner and the variation selector), are dropped. A line with no letter
  or digit left (a character of Unicode category L or N) is dropped. A task-list marker (`[ ]`, `[x]` or
  `[X]`) is dropped and blanks are collapsed.
- For a reply **still arriving** (`streaming`), what an unfinished construct would still change is held back, so
  the text spoken so far is always the start of the text to come:
  - only the last line of the Markdown is looked at; none of this applies when the number of lines that start a
    code fence (three backticks or three tildes, at the start of the line) is odd, that is, inside an open
    fence, where code is not read anyway;
  - a backtick turns code on or off, and nothing is looked for inside code;
  - a `[` opens a bracket, which a `]` not followed by `(` closes, and one followed by `(` closes at the next
    `)`; the text from a bracket that is still open is held back (a link whose words or address are not
    complete), and from the first of them when two are open;
  - a `**` or a `__` is skipped as a pair, and the text from a lone `*` or `_` that is still open is held back; a
    mark alone between blanks, a bullet, and an underscore inside a word (with a letter or a digit on both sides:
    "snake_case", "item_2") are not emphasis;
  - bold and code spans are closed as in the Markdown rules.
  The text **ends with a newline when its last line is finished**: a block follows it (a code block, a table or
  a rule included), or the Markdown ends with a blank line (which may hold blanks and tabs).

**Sentence chunker.** It takes the speech text so far and says which sentences became whole. Lengths count
Unicode scalar values and a blank is a character with the Unicode `White_Space` property. Stops, closers,
abbreviations and the two limits are `x-clientPolicy.voice` (`sentence...`).

- A sentence ends at a stop of `sentenceStops` (and any closer of `sentenceClosers` after it) when a blank
  follows, or at the end of a finished line; at a stop of `sentenceWideStops` at once. A full stop does not end
  a sentence in a number, after an abbreviation of `sentenceAbbreviations` (compared in lowercase, and only
  when the abbreviation ends in a single stop: "Dr. Smith", "Av. Paulista", but "Dr.. Smith" ends), or when the
  next word starts in lowercase, any Unicode lowercase letter ("5 p.m. today", "etc. and", "etc. épicos"); a
  full stop with a blank and nothing after it yet waits for the next character.
- A line ends at a newline, and sentences of two lines are never joined.
- A sentence under `sentenceMinChars` (a short pause such as "Yes.") is not spoken alone while another sentence
  of its line may follow: it waits and is joined to the next one. The last sentence of a finished line is
  spoken as it is, however short.
- A sentence is trimmed of blanks: the characters of Unicode category Zs (the no-break space among them) and the
  tab; a newline is not one, as no sentence holds one.
- A sentence over `sentenceMaxChars` is cut after its last `,`, `;` or `:` **that has a blank after it** before the
  limit, else at the last blank, else at the limit. An unfinished tail that outgrows the limit is spoken in parts (the blanks it starts with are not a part, and
  a sentence of nothing is never spoken).
- When the reply is final, what is left is spoken as it is.
- When the text is not an extension of the one before, cutting goes back to the end of the last sentence that
  both texts begin with.

**Reply feed.** A reply is one message of the avatar and has an identity (`id`). Updates with the same `id` are
one reply; another `id` is another reply, spoken after those before it, whether or not a voice turn preceded
it. A reply that was cancelled (the person talked over it or interrupted it) is never spoken again: the late words
that still arrive for its `id` are dropped, and they say nothing of the agent's current turn (the loop must not
take them for the reply it is waiting for, nor end its wait because of them: an update reports whether it was
accepted). **A new message id from the agent turn that was cancelled is the pane's to drop**: the loop knows no
more than the ids it has seen, so after `onCancelReply` the pane gives it nothing of the cancelled turn, new ids
included. A reply has more to come until its final update. The feed remembers the last `repliesRemembered` (64) identities
and forgets the oldest: a late update of a forgotten identity is a new reply. A reply that has had its final
update stays final. Each update is read as a text that may still grow (`streaming`) until its final one.

**Heard.** What the person heard of the reply they interrupted: the sentences played to their end, and the
part of the one playing that its position in the audio covers. With the voice's own word timings (the caller passes the words' timings, not the phonemes'), a word is
heard once the next one has begun, that is, when that word's `start` is at most the position (the last one when
all of the sentence was heard, a share of 1; a share of 0.95 does not). The words heard are counted, and that
many are taken from the start, so timings out of order count each heard word and cut at that number. Without
timings, or with none that are words, the share played selects the whole words: it is clamped to 0 to 1 (a share
that is not a number is 0), a word being a run of non-blank characters and each character of a script with no
blanks a word of its own; the text is cut after the last whole word (half of three words is one), and the cut
is trimmed of blanks as a sentence is.

**The vector file** (`speech-text.json`) holds `policy` and five groups of cases, each a list of objects with
these keys and no others; a missing or unread key fails a client's test.

| Group | A case | How it is run |
| --- | --- | --- |
| `speechText` | `name`, `markdown`, `spoken`, and `streaming` (true, or absent for false) | `speakable(markdown, streaming)` is `spoken` |
| `chunker` | `name`, `updates`, and `config` (`minimumLength`, `maximumLength`, either or both) | one chunker, made with the policy's limits over `config`, takes each update in order |
| `stream` | `name`, `updates` | one chunker with the policy's limits takes each update's `speakable(markdown, streaming = !final)` and its `final` |
| `replies` | `name`, `steps` | one reply feed takes the steps in order |
| `heard` | `name`, `played`, `current` (a string or null), `fraction`, `heard`, and `timings` with `position` (both or neither) | the heard part of the arguments is `heard`; `position` is 0 without timings |

An update of `chunker` is `spoken` (all the speech text so far), `final` and `emit` (the sentences that update
returns, in order); an update of `stream` is `markdown`, `final` and `emit`. A step of `replies` is either
`cancel` (true: every reply seen so far is cancelled) or `update` (`id`, `markdown`, `final`) with the `emit` and
`accepted` the update returns; both kinds of step carry `pending`, what the feed says after it. A `timings` list
holds objects of `text` and `start` in seconds, the words of the sentence playing; an empty one is no timings.
The feed of a case is made new for it, and so is each chunker. A no-break space in a case is written as its
escape.

## 4. The client (lands with the second and third changes)

This section says what the models and the audio of the Apple clients must do; the code arrives with those
changes (the catalogue of the models, `VoiceAudio` and `VoiceLoopController`), and so does the list of privacy
strings in `docs/app-store.md` that section 5 names.

`VoiceActivityDetecting` (the speech probability of a 512-sample frame at 16 kHz) and `EndOfTurnDetecting`
(whether the last 8 seconds of the turn are a finished turn) are the two model seams, listed in the
catalogues of `apps/dictation` (`VoiceModelCatalogue`) like a dictation engine or a voice: a descriptor with
its license and `commercialUse`, an install that is the person's to start, and a fallback that needs no
model (loudness; the pause alone). A model that cannot answer **throws**: that is not a silence. The loop
reports it, and goes on with the loudness detector, or ends the turn at the pause.

| Model | Runs as | License |
| --- | --- | --- |
| Silero VAD 6.2.1 | CoreML, run directly (its own state per stream) | MIT |
| Smart Turn v3.2 | CoreML, run directly | BSD-2-Clause |

Both are downloaded from a pinned commit of the model host, never from its moving branch, each file checked
against its size and SHA-256 (`PinnedModelDownload.swift`; the commits are the `version` of
`scripts/third_party/models.json`). An installed model is loaded from disk with no network.

**One engine owns the microphone.** The audio is `VoiceAudio`: one engine that records through the system's voice
processing (echo cancellation) and plays the avatar's speech, so a person can talk over the avatar through the
loudspeaker. A dictation does not compete with the loop for the device:

- a dictation that starts while the loop runs joins the loop's session;
- while a capture that the loop did not start is joined, the loop feeds its machine silence (a probability of 0
  for every frame), so the dictated speech is not taken for a turn, and the avatar may speak meanwhile;
- a dictation that runs on its own engine when the loop starts is moved onto the session, and is handed back to
  its own engine, or ended with an event, when the loop stops;
- a capture that is dropped without having been stopped does not keep the device from the loop, nor keep the
  loop deaf;
- a recognizer of the loop never starts from audio before the moment a dictation left (the end of a dictated
  word is not the start of a turn), and a capture that is moved keeps the format of its first buffer, so a
  recognizer never sees the audio change format in the middle of a request.

Only one loop runs in a process. The loop cancels its own recognizers before the audio stops. The audio
stopping by itself (a device removed, an interruption, a reset of the system's audio services) ends the loop with
the reason and with `stop`: playback and listening end and the agent's work is **not** cancelled. So does a
microphone that delivers no frame for `watchdogMs`, or none at all within `firstBufferMs` of the start (the
machine's own sleeping, or a stop in a debugger, does not count).

**Work off the main actor**: the microphone's samples are framed and read by the voice model on a queue of
their own, which also keeps the last eight seconds in a ring; the main actor gets the results. The loop stops
the microphone if its controller is dropped.

The stage reads, from `VoiceLoopController`: `state`, `phase`, `inputLevel`, `outputLevel` (the loudness of what
plays, about every `outputTickMs`), and `currentSpeech` with `playbackPosition` (the audio of the sentence being
heard, with the voice's own word or phoneme timings when it has them). The pane gives it each message of the
avatar with its identity (`replyUpdated`), and says when a turn's reply ended with nothing to say
(`replyEnded`); it is told when the person's turn goes out and when a reply is no longer wanted.

## 5. Privacy

The strings and the privacy answers of the apps must change **before the avatar pane ships**; nothing is wired
to the interface in this slice, so they are not changed here. The microphone is open from the start of a
conversation to its end, not while a button is held, and where on-device recognition is not supported a probe
sends audio to the system's recognition service. `docs/app-store.md` ("The voice loop: strings and answers to
change before the avatar pane ships", added with the third change) lists what to change.
