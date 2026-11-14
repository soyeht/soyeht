# ADR 0166: A workspace is a project with one avatar agent

- Status: proposed (phase 1: this design and its estimate only; no product code. The owner decided on
  2026-11-14 that it is required for 0.1.0)
- Date: 2026-11-14
- Builds on ADR 0083 (dictation engines are a catalogue), ADR 0148 and contract 17 (an agent delegates
  to helper agents in panes), ADR 0137 (a pane runs the Claude Code the person has), ADR 0154 (a
  workspace is a folder), ADR 0161 (the protocol only grows), ADR 0149 (only permissive licenses
  ship), ADR 0087 (agent reply Markdown), contract 08 section 2 (roles, `chief`) and the web pane
  (contract 11, ADR 0109, ADR 0110). The animation comes from the standalone avatar lab (outside the
  repository, see "What exists today").

## Context

The owner's description, 2026-11-14. Every workspace is a project of any kind: reading a book with an
agent that marks it and makes flashcards plus listening, writing and voice exercises shown on screen;
personal finances; an app; a work project; a school subject. Each workspace has **one avatar agent**,
configured like the voice screen of the lab: the harness (the thinking engine: Claude Code, Codex,
OpenCode, other models), the speech-to-text and the text-to-speech (cloud or local, light models, many
providers pluggable). The avatar is a 2D photo-like picture: a suggested preset, an upload, or a
candidate the chosen harness generates with an image model (the owner will rank the suggested image
models later). An agent then generates every frame the animation needs from that picture. The person
talks to the avatar by voice or by text. Avatars are saved and reused across workspaces.

Principles from the owner that bind this design: agents are smart, so Soyeht builds plumbing and not
behaviour an agent does from plain instructions; no security ceremony beyond what protects the person;
Apache-2.0 for what we write and no GPL code linked.

This is the largest feature left before 0.1.0 and it touches every client, so this ADR also gives the
honest scope and the estimate the launch date has to be re-planned on.

## What exists today

### In the repository (origin/main)

| Need | What exists | Where |
| --- | --- | --- |
| Workspace | `Workspace {workspaceId, name, path, layout}`; a workspace is a folder, and a missing folder is a state (ADR 0154); `workspace.create` requires a `path` | `contracts/protocol.json`, ADR 0154 |
| Panes | `PaneMode` is `terminal, agent, editor, web, writing`; `pane.create` takes `harness`, `model`, `effort`, `permissionMode` | `contracts/protocol.json`, `docs/add-a-pane-type.md` |
| Agent panes | ACP session over the person's harness; `pane.prompt` (text, attachments, origin `human` or `automatic`), `pane.update` events (agent message, thought, tool call, plan, usage), `pane.turn_ended`, `pane.cancel`, permissions answered from any client | contract 04, contract 05 |
| The lead agent | role `chief` (`pane.role.set`), `task.dispatch`, helper delegation in panes of their own, `agent.message` between panes | contract 08 section 2, contract 17, ADR 0148, ADR 0165 |
| Harnesses | `harness.list`, custom harnesses, per-pane model, effort and mode; Claude Code, Codex, OpenCode, Cursor | contract 01, 02, ADR 0137 |
| Agent tools | MCP: messaging, tasks, loops, writing, and `web_open` with `web_snapshot`, `web_click`, `web_evaluate` and the rest (a web pane an agent drives) | `engine/soyeht-mcp/src/tools.rs`, contract 11 |
| Skills | one skill store for every harness (`SKILL.md`), with the `/` picker | `engine/soyeht-skills` |
| Dictation | a catalogue of `SpeechRecognizing` engines (Apple, Nemotron 3.5 streaming, CoreML), install state, vocabulary, language, shortcut; Mac, iPad and iPhone | ADR 0083, `apps/shared/.../Dictation*.swift`, `apps/dictation` |
| Mic capture | `MicrophoneCapture`, `AudioResampler`; the Mac and iOS entitlements and usage strings exist | `apps/shared/.../MicrophoneCapture.swift`, `apps/mac/Soyeht.entitlements` |
| Credentials | a vault for secrets on the Apple clients | `apps/shared/.../CredentialVault.swift` |
| Android | a Compose view over a Rust core (deferred for 0.1.0) | ADR 0134 |

What does **not** exist: any text-to-speech, any audio output, any voice-activity or turn detection,
any avatar or animation code, any per-workspace configuration (settings are global keys), any audio
capture on the Windows and Linux app (its Dictation panel is listed with a notice), any audio or ONNX
crate in the engine or `apps/desktop` (their lock files hold none), and any way to open a project file
as a web page without a running server. The Mac has no "voice screen" other than the dictation
Preferences panel; the voice screen the owner means is the lab's.

The design file `design/soyeht-desktop.pen` was not opened (.pen files are encrypted and only the
Pencil tool reads them, which needs the app). The concept screens are known from the owner's notes:
"Tela 20 · Super-agente por workspace" (a tab is a super-agent with a name and an avatar, the left rail
holds its apps, the harness is the engine), "Tela 26" (the avatar speaking) and "Tela 27" (choosing and
creating the avatar). Nothing in this ADR contradicts them; the screens are to be rechecked in phase 2.

### In the lab (`/Users/macstudio/Documents/soyeht-avatar-lab/`, not part of the repository)

The lab is one local web page that imitates a Soyeht pane holding a talking avatar (`README.md`
is the map). "The voice screen" is its **Laboratorio panel** (`static/index.html`, `static/app.js`):

| Selector | Options |
| --- | --- |
| Pipeline | "Simple" (mic, STT, brain, TTS per sentence, avatar) and plug-ins for Pipecat, Hugging Face speech-to-speech and LiveKit (`static/pipelines/`, `frameworks/`) |
| Avatar | Sabio 2D (whole-frame swapping plus acting), Sabio rig 2D, a mascot, a 3D VRM, voice only, NVIDIA 3D heads |
| Lip sync | HeadAudio (MFCC visemes on the playing audio), volume, TTS timings, NVIDIA Audio2Face-3D |
| STT | streaming: Nemotron 3.5 and Apple SpeechAnalyzer (Swift helpers in `stt-helper/`); batch: mlx-whisper, parakeet-mlx, and whisper-turbo and parakeet-v3 on bignix |
| Brain | Claude Code, Codex and OpenCode as headless CLIs (`llm_cli.py`), OpenCode Go API, ChatGPT sign-in (`api_brains.py`, `chatgpt_auth.py`), exposed to frameworks as an OpenAI-compatible endpoint |
| Voice | Kokoro-82M on the Mac (MLX) and bignix, a Portuguese Chatterbox on bignix, Kokoro in the browser (English only) |

It also holds the hands-free conversation (`static/conversation.js`: Silero VAD v5, Smart Turn v3.2 in
`turn_model.py`, barge-in with continuer and echo rules, push to talk), vision (screen and camera
frames with a per-model "can see" probe, `vision.py`), the Qwen frame jobs (`tools/sabio_*.py`,
`tools/qwen_batch.py`, run on bignix through ComfyUI) and the rig (`rig/`, Apache-2.0).

Measured by the lab, with the harness CLI as the brain: Pipecat round trip about 3.7 s for a short
reply and 5.8 s typically, 12.9 s for a long one (`logs/pipecat-latency.jsonl`); the CLI's own
3.0 to 5 s dominates. Server compute on the RTX 4090: Kokoro 80 to 150 ms for 4.8 s of audio, parakeet
35 to 115 ms, Audio2Face 85 to 115 ms (`~/avatar-lab/README.md` on bignix).

### Which animation is the validated one

**H0, the continuous mouth** (`docs/mouth-continuous.md`; runtime `rig/runtime/mouth.js`; mouth mesh
`rig/author/mouthmesh.py`). Five parameters (opening, width, rounding, closure, lip-teeth contact)
follow phoneme targets with critically damped motion; the geometry is a morph between the lip contours
of nine whole-frame drawings, so exactly one mouth is ever visible. It is used by two avatars: **A**,
the frame-swap Sabio (a base picture, nine mouth drawings, a blink, seven eye and two resting-mouth
frames, a head warp and the acting layer in `static/sabio-acting.js`, `sabio-warp.js`) and **B**, the
layered rig (23 parts, hair physics).

How I know it is the validated one, from the lab docs and the owner's recorded verdicts:

- 2026-11-05: the owner rated the animation of the 2D Sabio 8+/10 and the first mouth (swapped and
  cross-faded drawings) 1/10. That produced H0.
- 2026-11-05: **H1** (`docs/mouth-h1.md`) was rejected, 2/10, worse than H0 in both avatars: a second lip
  line drawn as a separate stroke layer. 2026-11-06: **H1b** fixed that and still lost to H0 (flat white
  strip instead of drawn teeth, calmer and with almost no rounding). "H0 stays."
- H0 passes every limit of `mouth/check_mouth.py` (no ghost frames, largest one-frame change 0.62 to
  0.65 of the closed-to-A distance, M/B/P closed 100 %, silence closed 100 %). The lab's own notes and the
  owner's feedback agree that a checker pass is not evidence; the owner's choice of H0 over two
  challengers is.

Caveats the estimate depends on: I found **no recorded score by the owner for H0's mouth itself**
(it is validated by being the one he kept); it was validated on **one painted illustration**, never on a
photo-like face; the rig's authoring (`rig/author/*.py`) is Python and Sabio-specific in its layer
rules; and nothing in the lab runs outside a browser. Avatar **A** needs far less authoring than **B**
(no layer split, no inpainting, no hair physics) and has the same mouth, so it is the launch tier.

## Decision

### 1. A workspace is a project; the avatar agent is its chief agent pane

- No new pane mode. The avatar agent is the workspace's agent pane with role `chief` (contract 08),
  started by Soyeht when the workspace has an avatar. It thinks with the harness the person chose
  (`pane.create` already takes harness, model, effort and permission mode), keeps one ACP session, and
  delegates to helper agents (contract 17). That one pane is the brain for both voice and text:
  voice and text are two ways to write to the same conversation, and the transcript is the same
  `pane.update` stream the agent pane view already draws.
- The brain of the avatar is **never a separate process** as in the lab (`claude -p` per turn). The
  persistent session removes the cold start, keeps the context, and lets the agent use its tools.
- A workspace of any kind needs a folder, because the engine, the editor, the agents' working
  directory and ADR 0154 assume one. "New project" without a folder is a client flow over
  `workspace.create`: the client asks the engine for a new folder under the data directory's
  projects folder (named from the project name) and passes it as `path`. No protocol change.
- What the agent does in a project (marks a book, builds flashcards, tracks spending, opens a panel of
  exercises) is **the agent's job from plain instructions**. Soyeht adds only: the persona text below, the
  existing tools, and the one gap named in section 9 (showing a project file as a page).
- The persona is the avatar's name, a short statement of how it speaks in a conversation (one to three
  spoken sentences, no Markdown, it may open panes to show things) and the person's own instructions.
  It is plain text stored with the workspace binding and given to the agent when the pane starts, through
  whatever instruction channel the harness has (a file in the project folder the harness reads, or the
  first prompt of the session). Which one per harness is a slice-2 decision made by measurement.

### 2. What is stored, in the engine and additive (ADR 0161)

Two records in `soyeht-store` (a new migration; only that crate has SQL), three groups of methods in
the protocol, no change to an existing method or type:

- **Avatar** (the reusable library, global to the person): id, name, `kind` (`preset`, `uploaded`,
  `generated`), the hash of the source picture, the build state (`none`, `building`, `ready`, `failed`
  with a reason kind), the package hash, the default voice (TTS provider id, voice id), a created date.
  Files live in `<data>/avatars/<id>/`: `source.png`, `package/` (below), and the build log.
- **Workspace binding** (one per workspace): workspace id, avatar id, the harness, model and effort
  (so the avatar pane can be restarted the same way), the STT provider id and language, the TTS provider
  id and voice (overrides the avatar's default), the persona text, and "voice on" per client.
- Methods (names proposed, `avatar` is a new namespace): `avatar.list`, `avatar.create`,
  `avatar.rename`, `avatar.delete`, `avatar.assign` (binds an avatar and the voice and harness settings to a
  workspace and starts its pane), `avatar.build` (starts a build job), `avatar.package.read` (ranged
  read of a ready package, so a phone can fetch it; the existing `attachment` frame carries bytes up to
  `attachmentPayloadBytes`), and events `avatar.changed`, `avatar.build_progress`. Limits (counts, sizes,
  name length) go in `x-limits`, never typed. The persona and provider settings are never log text.
- **Provider keys do not travel and are not stored by the engine.** Voice providers run in the client
  that has the microphone (section 5), so each client keeps its own provider keys in its own vault
  (`CredentialVault` on Apple; the desktop's own store). Image providers run where the avatar is built, on
  the engine host, with keys in the engine host's vault. A phone that uses the avatar needs no image key.
- Settings are global today; per-workspace values live only in the binding. The generated clients
  (`contracts/generate.py`) carry the new types; every wire name comes from the generated forms.

### 3. Configuration flow (the lab's voice screen, productised)

One screen per client, the same order everywhere (Mac and iPad sheet, iPhone full screen, desktop view):

1. **Picture**: choose a preset, upload a picture, or "generate" (section 4). A preset is an avatar
   that ships already built (its package is downloaded on first use), so the common path needs no image
   provider, no GPU and no wait.
2. **Brain**: the harness, model and effort, with the existing harness picker (`harness.list`).
3. **Hearing** (STT) and **Voice** (TTS): a provider catalogue (section 5), a language, a test button.
4. **Name and persona**.

A configuration with no cloud account and no install must work: Apple's speech on the Apple clients, and a
preset avatar. Everything else is opt-in and says what leaves the device (section 8).

### 4. Where the picture and the frames come from

**The picture.**

- Presets: 6 to 8 photo-like synthetic people generated by us, with their packages built, shipped as a
  downloadable asset pack (not in the app binary). They depict no real person. The image model's terms for
  commercial use of its outputs are checked before the pack ships (slice 9).
- Upload: any picture of a face. The person is responsible for the likeness of whoever it shows; the
  screen says the picture is sent to the image provider when one is used.
- Generate: the avatar agent (the workspace's harness, or any chosen one) makes candidates with the image
  tool it has or with an image provider of Soyeht's catalogue (below), and the person picks one. The
  list of suggested image models is data (a small JSON in the app, ranked later by the owner), not code.

**The frames.** "An agent generates every frame" is built as three pieces, the first two plumbing and
the third plain instructions:

- **Frame spec**: the exact list for the launch tier (avatar A, from the lab's `tools/sabio_spec.json` and
  `tools/sabio_acting_spec.json`): 10 base frames (rest, closed M/B/P, A, E, I, O, U, F/V, L/D/T, blink),
  7 eye frames (half, left, right, up, down, up-left, soft) and 2 resting mouths (smile, "hm"), 19 images in all. Each has a prompt
  template and an acceptance rule (same identity, same framing, only the named region changes).
- **`soyeht avatar` commands** in `soyeht-cli` (it already depends on protocol, platform, ptyd, service
  and store, and the commands are a thin layer): `frame` (run one image edit through a provider adapter),
  `check` (a deterministic check: size, framing, the difference outside the named region, face landmarks
  found), `build` (landmarks on each frame, the mouth mesh, the changed-rectangle diffs, the atlas, the
  package, validation) and `status`. The adapters: an OpenAI-compatible image-edit endpoint, a ComfyUI-style
  endpoint (what the lab used on bignix), and the harness's own image tool as a manual path.
- **A skill**, `avatar-builder` (a `SKILL.md` in `soyeht-skills`, so every harness has it): it tells the
  agent to take the picture, call `frame` for each pose, look at the results, redo the ones `check` or its
  own eyes reject, and then call `build`. The agent runs as a helper in its own pane (ADR 0148), so the
  person sees it work, and `avatar.build_progress` is derived from its frame count.

Time and cost per avatar (the lab's measurements, and what is not measured):

- Local on an RTX 4090: Qwen-Image 2.1 edit, int8, 25 steps, **40 to 60 s per image** (from the ComfyUI
  logs on bignix), so one pass of 19 images is about 15 to 20 minutes, and 30 to 60 minutes with the
  rejected frames redone (the lab needed several seeds per mouth). The model file is 7.3 GB int8.
- Cloud image provider: not measured. Expect seconds per image and a few cents per image, so a few dollars
  per avatar including redos; the spike (slice 0) measures it on the providers we list.
- **A machine without an NVIDIA GPU** (a Windows laptop, the owner's Mac) does not run Qwen locally in this
  design: nothing in the lab runs it on a Mac or on a CPU. The answer is the cloud provider (the
  person's key) or a preset. Local generation on a capable computer of the person's own (the lab's bignix
  case) is the same ComfyUI adapter pointed at that computer.
- The build step after the frames (landmarks, mesh, atlas) is CPU work of seconds to a minute.

### 5. The voice loop runs in the client that has the microphone

The lab and an earlier note put the speech models on the computer and sent the phone's audio to it. This
design reverses that: **speech runs on the device in the person's hand, and only text crosses the wire**.

```
mic -> AEC -> VAD -> turn detection -> STT (streaming) ---text---> pane.prompt
                                                                     |  (the avatar pane, on the engine host)
speakers <- TTS (per sentence) <- sentence chunker <-----text------- pane.update (agent message)
                 |
                 +-> phonemes + audio clock -> mouth parameters -> avatar renderer
```

Why: the audio devices are there; no audio protocol, frame kind or bandwidth appears in the wire; barge-in
is local and instant; the phone works against a Mac with no model on the Mac; and the engine stays free of
audio, which the platform seam would otherwise have to carry for three systems.

What is plumbing (built once per client language, held together by the same vector files, as ADR 0087 and
the layout rules are):

- **Turn state machine**: idle, listening, thinking, speaking; the lab's rules from
  `static/conversation.js` (speech start after voiced frames with pre-roll, an end-of-turn check after
  350 ms of silence, barge-in at 350 ms of speech, continuers and echo ignored, the interrupted reply kept
  in the conversation with the heard part). Vectors: `contracts/vectors/voice-turns.json` (new, written first).
- **Sentence chunker and speech text**: cuts the streaming agent message into sentences for TTS, and turns
  Markdown into speakable text (the shared Markdown parser of ADR 0087 gives the structure; code blocks and
  links are not read aloud).
- **Aligner and mouth driver**: described in section 6.
- **Providers** (catalogue pattern of ADR 0083: a descriptor, a registry, nothing knows an engine by name):
  `SpeechRecognizing` exists; `SpeechSynthesizing` is new; a `VoiceActivity` and an `EndOfTurn` seam so the
  VAD and the turn model are replaceable. Launch set:

  | | Apple (Mac, iPad, iPhone) | Windows and Linux app |
  | --- | --- | --- |
  | STT | Apple speech, Nemotron 3.5 streaming (exist) | local: a Whisper-class model through whisper.cpp or ONNX, CPU first, GPU when present; cloud |
  | TTS | Apple voices; Kokoro-82M on CoreML (English first, see licenses) | local: Kokoro (English); the system voices where the system has good ones; cloud |
  | Cloud | one generic provider over the **OpenAI-compatible audio endpoints** (a base URL, a key, a model), which covers OpenAI, Groq and the many that copy the shape; ElevenLabs and Deepgram as the first two native adapters | the same |
  | VAD, turn | Silero VAD v5 and Smart Turn v3.2 (ONNX), CoreML or onnxruntime | the same ONNX files through `ort` |

- **Echo and barge-in per system.** Apple: the audio engine's voice processing (echo cancellation) allows
  full-duplex barge-in over speakers, as in the lab. Windows and Linux: **half-duplex at launch** (the mic is
  gated while the avatar speaks and a key or click interrupts it; headphones recommended) unless the AEC
  spike of slice 8 passes. This is a stated limit, not a hidden one.
- **The phone** runs its own STT and TTS and talks to the Mac's avatar pane over the pairing it already has.
  An iPhone that cannot run the optional local models uses Apple's or the cloud's.
- **Latency.** End of speech to first sound is the sum of the end-of-turn wait (0.2 to 0.4 s), STT final
  (about 0.3 s streaming), the harness's first sentence, and TTS first audio (0.1 to 0.5 s). The harness
  dominates: the lab saw 3.0 s with a short answer; a persistent ACP session should remove the process start
  but not the model. **Target: P50 5 s with a subscription harness, 2 s with a fast model** (a custom
  harness or OpenCode with a flash model). The avatar's "thinking" state (the lab's eyes up-left and "hm"
  mouth) carries the wait. Whether a spoken acknowledgement while thinking is worth it is the agent's call
  from the persona, not a feature. The slice-0 measure replaces these numbers.
- **Interruptions and tools.** A barge-in is `pane.cancel` followed by the person's words as the next
  prompt, with the part the person heard. A permission request from the avatar's agent is spoken and
  answered by voice ("yes", "no") or on screen; it uses the existing permission events.

### 6. The animation: one package, one runtime per client, a continuous mouth

**Package** (`avatar package`, a folder of a manifest, PNG atlases and one JSON, about 0.5 to 1.5 MB, the
lab's rig is 437 KB plus its atlas): the base picture, the mouth model (the shared vertex topology of the
nine contours, the skin, inner-mouth and lip parts), the eye and blink frames with their rectangles, the
warp lattice of the head, and the acting constants. Versioned (`format`), validated by `soyeht avatar build`
and by the client on load. Apache-2.0, our own format (the owner's decision of 2026-11-04); Live2D, Spine
and Inochi2D are references only.

**Runtime** (pure, no IO): the five mouth parameters, the phoneme-to-target table, the damped motion and
slew limits, the morph weights (a convex combination, so the weights sum to 1), the acting layer (idle,
listening, thinking, speaking; blinks, saccades, breathing, head warp). It is a port of
`rig/runtime/mouth.js`, `driver.js`, `core.js` and `sabio-acting.js`; the lab's math is already pure.

- Rust: a new crate `soyeht-avatar` (layer 3, depends on nothing but `soyeht-protocol`, no IO, no async),
  used by the desktop and by `soyeht avatar build` (the same loader, validator and mouth mesh builder).
- Swift: the same runtime in `apps/shared` (`SoyehtClient`), used by the Mac and iOS.
- **The two implementations are held together by vectors** (`contracts/vectors/avatar-mouth.json`: a phoneme
  track in, the five parameters at each 1/60 s out, within a tolerance; and the package load rules), the
  same method the repository uses for the Markdown subset and the layout floors. Written first.

**Continuous mouth is a hard requirement** (the owner, 2026-11-05): the mouth is parameters and geometry,
never a step between pictures and never a cross-fade of two mouths. The runtime has no code path that swaps
mouth drawings. The acceptance for any change to it is the lab's checker limits ported to a test (ghost
frames 0, largest one-frame change at most 0.65, M/B/P closed at least 90 %, silences closed) **and** a
contact-sheet look by a person across the whole clip, because H1 passed the checker and was rejected.

**Drawing.** The lab draws with WebGL. Soyeht draws by textured triangles from the same parameters. The
reference implementation is a small software rasteriser (about 512 px, a few thousand triangles, one core),
identical in Swift (a CGImage layer) and Rust (an RGBA buffer painted as an image in GPUI), with a GPU
path (Metal; wgpu through GPUI) left for later if the spike measures a cost. The rasteriser is what keeps
one algorithm on three renderers; its frame cost on a Windows laptop is a slice-0 measurement.

**Mouth input**, in three tiers, so any TTS works:

1. phoneme timings from the TTS (local Kokoro with its predicted durations; a cloud adapter that returns
   character or word alignment);
2. **text plus the audio envelope**: the known text becomes phonemes through our own rules and is aligned to
   the voiced span of the audio actually playing (the lab's words-only fallback closed M/B/P 88 %, nearly
   passing; the aligner is slice 3's work item);
3. energy only (the floor).

HeadAudio-style MFCC visemes are not used: they are trained on English and moved the mouth little on
Portuguese in the lab.

**Not at launch**: the layered rig (avatar B), 3D (VRM, NVIDIA heads, MetaHuman), neural talking heads,
screen and camera sharing from the lab.

**Photo-like pictures are the unvalidated part.** The validated mouth morphs nine **painted** drawings
whose teeth and tongue blend acceptably; photographic teeth, skin texture and lighting may ghost or look
uncanny when averaged. If the slice-0 spike fails the owner's look test on photo-like frames, the launch
falls back to illustrated avatars (presets and uploads converted to the illustration style by the image
tool), which is the case the lab proved. The estimate does not change; the product claim does.

### 7. Text chat with the same avatar

The avatar pane is the agent pane (same conversation, composer, attachments, dictation, permissions) with
an **avatar stage** above the transcript on every client: a header strip on a phone, a column in a split.
Voice off, the person types and the avatar animates the reply with TTS only if "speak replies" is on, and
mutes if the stage is hidden. Nothing is duplicated between the voice and the text path: voice writes to
the same `pane.prompt` that the composer writes to, with origin `human`.

### 8. Privacy, licenses and the review of the Apple apps

- Default is local or Apple. A cloud STT, TTS or image provider is chosen by name, shows what it receives
  (audio, text, the picture) before the first use, and is described in the App Store privacy answers
  (`docs/app-store.md` section on data) and in the microphone and speech usage strings. Provider keys stay
  in the vault of the device that calls the provider.
- Models are downloaded at runtime after the person presses Install, as ADR 0083 does for Nemotron: nothing
  is bundled, nothing downloads by itself, the install shows size and license. Every library that links
  must pass `scripts/third_party_notices.py` (ADR 0149).
- A model whose license is not permissive is not offered as a built-in; it may be reachable only as a
  user-added custom endpoint.

### 9. The gap for "shown on screen"

The owner's examples show flashcards and exercises on screen. Agents can already write a page into the
project and open a web pane to a server; I found no way to open a project file as a page without starting
a server. The cheapest plumbing is to let `web_open` take a project-relative file path that the engine
serves read-only on loopback for the life of the pane (the page's own scripts may call the agent through
nothing: it is a view; the agent edits files and the page reloads on change). Slice 13. The agent's own
ability to design the exercises is not built.

## Minimum for launch, and what comes later

**Launch (0.1.0), required**: workspace with an avatar agent as the chief pane; avatar library with presets
and upload; generation by an image provider (cloud, the person's key) with the build agent; avatar A with
the H0 continuous mouth on the Mac, iPad, iPhone and the Windows and Linux app; voice loop with Apple speech
on Apple and a local or cloud STT on Windows and Linux; TTS with Apple voices, Kokoro English, and the
OpenAI-compatible cloud adapter; text chat; saved and reusable avatars; the project-file page; half-duplex
voice on Windows and Linux; full-duplex barge-in on Apple.

**Later**: the layered rig and hair physics (avatar B); local image generation as a packaged feature; neural
and 3D heads; Portuguese local TTS through a non-copyleft phonemizer; full-duplex echo cancellation on
Windows and Linux if the spike fails; more cloud adapters; sharing an avatar between people; an avatar with
its own apps in the rail (the lab's "Studies" and "Finances" idea) beyond what the agent builds from
instructions; Android.

## Licenses of what would ship or link

"Ship" means in the product or downloaded by it. Verified means read from a primary source in this phase
(the lab's own notes, or the project's page today); everything else is stated as unverified.

| Component | License | Verdict |
| --- | --- | --- |
| Kokoro-82M weights | Apache-2.0 (model card, verified) | Ship as a download. |
| espeak-ng, used by misaki and by sherpa-onnx's Kokoro and Piper paths for the phonemes of Portuguese and most languages | GPL-3 (the lab's own note says "not for shipping without review"; the voice package listings confirm espeak-ng data is GPL-3) | **Blocker for Portuguese local Kokoro**: cannot link or bundle (ADR 0149). English works with a dictionary phonemizer. A non-copyleft pt-BR phonemizer is a later slice; until then Portuguese local TTS is the system voice or a cloud voice. |
| sherpa-onnx (an option for STT, VAD, TTS in Rust) | Apache-2.0 for the project, but whether its Kokoro/Piper build statically links GPL espeak-ng code is **not verified** | Do not adopt before reading its LICENSE and build files; the plan uses `ort` and whisper.cpp directly. |
| Piper engine (`piper1-gpl`) | GPL-3 | Do not use. |
| Silero VAD v5 | MIT (the repository says MIT; a badge on the page also says CC BY-NC for the separate models repository; read the LICENSE file at the pinned version in slice 7) | Ship (pending that read). |
| Smart Turn v3.2 (Daily, pipecat) | BSD-2 (file headers in the lab) | Ship. |
| Pipecat design | BSD-2 | Design reference only; no dependency. |
| Whisper large-v3-turbo and whisper.cpp | MIT | Ship as a download. |
| Parakeet TDT 0.6B v3 | CC-BY-4.0 (attribution) | Ship as a download with the attribution. |
| Nemotron 3.5 ASR Streaming | OpenMDW 1.1 (ADR 0083) | Already shipped as an optional download. |
| Chatterbox and its Portuguese fine-tune | MIT (the lab's note); RTF about 1.0, no streaming | Not realtime; later. |
| Qwen-Image edit (the original) | Apache-2.0 (verified on its model card) | A local option only. **The lab uses "Qwen-Image 2.1" (`qwen_image_2.1_int8_convrot`); its license was not verified here**: read its card before the spike's report. The weights are never shipped; they run on the person's or their provider's machine. |
| Image providers (cloud) | Their terms | The outputs' commercial use must be allowed before presets ship (slice 9). |
| MediaPipe Face Landmarker and selfie multiclass models | Apache-2.0 (lab notes) | Ship (the build tool, as ONNX or the original format, conversion to be confirmed). |
| NVIDIA Audio2Face-3D | SDK MIT; weights NVIDIA Open Model License (commercial allowed, to be read); NVIDIA GPU only | Not in the plan. |
| HeadAudio, HeadTTS, three.js, three-vrm | MIT | Not in the plan. |
| TalkingHead avatars (`brunette`, `avaturn`, `avatarsdk`) | CC BY-NC or other non-commercial | **Never ship.** `mpfb` is CC0. |
| Live2D Cubism, OVRLipSync, Wav2Lip | Live2D's free tier excludes products that load user-made models; the others are non-commercial | Do not use. |
| See-through (layer decomposition) | code Apache-2.0, **weights license not stated** | Not a dependency (avatar B, later). |
| MuseTalk, LatentSync and other neural talking heads | not examined | Not tried; out of scope. |
| `ort` (onnxruntime), `cpal`, whisper-rs | MIT or Apache-2.0 (to be confirmed by the notices check when added) | Allowed class. |

## Risks

1. **Photo-like faces** are unvalidated (section 6). Mitigation: slice 0 first, with the owner's look test;
   fallback to illustrated avatars.
2. **Frame generation quality and cost.** Identity drift and mouth shapes the model refuses (the lab
   needed several seeds). Mitigation: the deterministic `check`, redo loops in the skill, a preset path with
   no generation, and a cost shown before the first build.
3. **No GPU** on most machines: local image generation is not offered to them; they use a cloud key or a
   preset.
4. **Latency with a harness brain** of 3 to 5 s or more. It is the harness's cost; the design hides it with
   the thinking state and gives the fast-model path.
5. **Windows and Linux audio**: no audio code exists there; echo cancellation is unsolved, so half-duplex at
   launch; device selection, sample-rate and permission handling are per system. This is the biggest
   engineering risk after the animation.
6. **Local speech models on the desktop** (CPU-only laptops): a Whisper-class model streams slowly on weak
   CPUs; the catalogue marks engines that do not fit and the cloud is the fallback.
7. **License traps**: espeak-ng (Portuguese local TTS), sherpa-onnx's build, the Qwen 2.1 card, Silero's
   file, provider output terms.
8. **Two implementations of the runtime** (Swift and Rust) can drift. The vectors are written first and
   both pass them in the gate.
9. **App Store review**: a microphone, speech recognition and cloud audio need accurate strings and
   privacy answers; an avatar of a real person's upload needs the likeness statement.
10. **Scope pull**: the lab holds much that is not in the launch tier (rig B, 3D, vision, frameworks).
    This ADR cuts it explicitly.

## Implementation slices

Sizes in agent-days: S 1, M 2 to 3, L 4 to 6 (the figure is the planning value). The gate and review time
of each slice is included. A slice names its tests.

| # | Slice | Size | Days | Depends on |
| --- | --- | --- | --- | --- |
| 0 | **Spike, gate for the whole feature**: three photo-like pictures through the Qwen jobs (or a cloud provider) into the 19 frames, the lab's avatar A with the H0 mouth, a software-rasteriser frame cost on a Windows laptop and the Mac, an ACP-pane brain latency measure. The owner looks and decides. | M | 3 | none |
| 1 | Contract 19 and the additive protocol (avatar library, binding, build events), store migration, engine methods, vectors, generated outputs | M | 3 | 0 |
| 2 | Workspace as project: folder under the data directory, avatar pane start as chief, persona delivery per harness, per-harness measurement | M | 3 | 1 |
| 3 | `soyeht-avatar` crate: package format, validator, mouth runtime, driver, aligner (tier 2), vectors `avatar-mouth.json` | L | 5 | 0 |
| 4 | Swift runtime and rasteriser in `apps/shared`, avatar stage view for Mac and iOS, vector tests | L | 5 | 3 |
| 5 | GPUI rasteriser and avatar stage on the desktop, frame budget on Windows | L | 5 | 3 |
| 6 | Voice provider catalogue (Swift): `SpeechSynthesizing`, Apple TTS, OpenAI-compatible cloud STT and TTS, Kokoro CoreML (English), key vault, install flow in ADR 0083's style | L | 4 | none |
| 7 | Voice loop (Swift): audio session with echo cancellation, Silero and Smart Turn through onnxruntime or CoreML, turn machine, barge-in, sentence chunker, speech text, vectors `voice-turns.json`; Mac and iOS | L | 6 | 6 |
| 8a | Desktop audio I/O (cpal), Silero and Smart Turn through `ort`, turn machine in Rust, half-duplex, AEC attempt (WebRTC audio processing or speexdsp) | L | 5 | 7 vectors |
| 8b | Desktop providers: local Whisper-class STT, Kokoro (English) TTS, cloud adapters, model install and notices | L | 5 | 8a |
| 9a | Image providers, frame spec, `soyeht avatar frame` and `check`, the `avatar-builder` skill, preset pack generation | M | 3 | 0 |
| 9b | `soyeht avatar build` in Rust: landmarks through `ort`, mouth mesh, diff rectangles, atlas, package; reproduces the lab's avatar A from its frames | L | 6 | 3, 9a |
| 10 | Configuration screens: Mac and iOS (SwiftUI), desktop (GPUI); picker, upload, generate and progress, harness, STT, TTS, persona, test voice | L | 5 | 1, 4, 5, 6 |
| 11 | The avatar pane: stage plus transcript plus composer on all clients, speak-replies toggle, voice on and off | M | 3 | 4, 5, 7 |
| 12 | Library: rename, delete, export and import a package file, reuse across workspaces, phone fetches the package | M | 2 | 1, 4 |
| 13 | A project file as a web page (section 9) | S | 1 | none |
| 14 | QA and hardening: voice matrix on real devices and the VMs, App Store strings and privacy answers, license notices, docs, preset licensing check | M | 4 | all |

Total of the slices: **68 agent-days** (3 + 3 + 3 + 5 + 5 + 5 + 4 + 6 + 5 + 5 + 3 + 6 + 5 + 3 + 2 + 1 + 4).

### Calendar estimate, three implementers in parallel, beside the other launch work

- Critical path: 0 (3) then 3 (5) and 9b (6) in parallel, then 4 and 5 (5), then 11 (3), then 14 (4): about
  **22 working days** with no slack, no rework and no other work.
- Add 25 % for integration, review, red gates and rework of a feature that crosses every layer: about 85
  agent-days of effort, 28 days of three people working only on this.
- The three people also carry the rest of the launch work (signing, installers, the update flow, QA of the
  other features). At 60 to 70 % on this feature: about **43 working days, 8 to 9 weeks**.
- If the work starts on Monday 2026-11-16 with the spike, **P50 is the week of 2027-01-11, and the range is
  2026-12-28 to 2027-02-05**. That is about **5 to 9 weeks later than the previous plan** (the launch checklist
  of 2026-11-06 said P50 3 December), because this feature does not overlap with the work that set that date.
- **Cuts, and why none moves the date much**: (a) Windows and Linux voice with cloud providers only, no
  local model there (slice 8b becomes M, about -2 days, and the CPU-speed risk goes); (b) no Kokoro on Apple,
  Apple voices and the cloud only (about -1); (c) voice on Apple clients only, with the Windows and Linux
  avatar answering text with TTS and push to talk (about -3); (d) presets and upload only, no generation by
  an agent (about -6, but the owner asked for generation). Together (a) to (c) save about 6 agent-days, which
  is about one week of calendar. The date moves by more than a week only by dropping a requirement the owner
  made (generation, or Windows and Linux voice) or by the spike failing.
- The slice-0 spike either confirms these numbers or replaces them within 3 working days. Do not announce a
  launch date before it.

## Questions for the owner

1. Is a launch with illustrated avatars acceptable if the photo-like look fails the spike?
2. Is half-duplex voice (headphones, a key to interrupt) acceptable on Windows and Linux for 0.1.0?
3. Is Portuguese local TTS on Windows and Linux required at launch? It needs a non-copyleft phonemizer
   (a later slice) or a cloud voice.
4. Which image providers go in the first list, and who pays for a build (the person's key)?
5. Presets: how many, and in which styles (photo-like, illustrated)?

## Consequences

- One agent pane is the avatar's brain for voice and text, so the avatar inherits every harness feature
  (subagents, tools, permissions) and its limits (latency, login).
- The wire gains a small avatar surface and no audio. Phones need no speech model on the Mac.
- The repository gains a pure crate (`soyeht-avatar`, to be placed in `scripts/check_layers.py` and the
  layer list of `AGENTS.md` when it lands), a Swift runtime, a CLI command group and a skill.
- Two implementations of the mouth and turn logic exist and are tied by vectors, the repository's rule.
- Windows and Linux get their first audio code; the cost is the largest single unknown after the animation.
