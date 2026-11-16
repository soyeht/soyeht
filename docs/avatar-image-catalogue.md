# The image catalogue and the frame tooling (`soyeht-image`)

ADR 0166, slice 9a. An avatar is one picture of a face and 18 edited copies of it (eight mouth
shapes, a blink, seven eye frames, two resting mouths). This is what makes the edits: a catalogue
of image providers the person chooses from, the adapters that send one edit to a provider, the
frame spec, and the commands that make and check one frame. The package build that turns the
frames into an avatar (landmarks, regions, mouth mesh, atlas) is slice 9b.

## Where it lives

`engine/soyeht-image` is a layer 3 building block (AGENTS.md): it depends on `soyeht-platform`
and `soyeht-process` and on no other building block. The command line is its only user today
(`soyeht avatar`, in `soyeht-cli`, which needs no running engine), and the engine can use it
later for the build job and for listing the catalogue to the clients (slices 1 and 10). It is not
in `soyeht-avatar`, which stays pure (no IO, no async runtime) because the renderers use it.

## The catalogue

One descriptor per provider, as data (`soyeht-image/data/catalogue.json`); the code reads the
list and never matches on an id or a model name. A descriptor holds:

- the id, the names, `locality` (`local`: a computer of the person's own; `cloud`: someone else's)
  and `adapter` (which protocol it speaks);
- the `model` and `endpoint` it starts with, both optional: the person gives their own for one
  call with `--model` and `--endpoint`. A cloud entry takes only an https address; a local one may
  take http. An address never holds a user name, a password, a query or a fragment;
- `needsKey`: whether the provider needs the person's own key;
- `sends`: what the provider receives (the picture, the instruction), which a screen names before
  the first use;
- `license`: a name, an optional address and `commercialUse` (false for research-only terms, absent
  when not known). It is data for the screens, and nothing enforces it;
- `timeoutSeconds` and `sizeMultiple` (both sides of the picture must be a multiple of it);
- for a ComfyUI entry, the `recipe`: the graph in its API form and the places the edit's values go.

The first list (the "Questions for the owner" item 4 of ADR 0166, decided here):

| id | where | adapter | notes |
| --- | --- | --- | --- |
| `qwen-image-2.1-comfyui` | local | `comfyui` | The recipe the project's own presets and tests use. The server is the person's own, at `http://127.0.0.1:8188` unless `--endpoint` says another. |
| `openai-compatible-image-edit` | cloud | `openai-images` | Any service that answers the OpenAI image-edit request. The person gives the address and the model and has their own key. |

Adding a provider is adding an entry; adding a protocol is adding an adapter (one file of
`soyeht-image/src/adapter/` and a value of `Adapter`).

### The adapters

Both send their requests through `curl` run by `soyeht_process::run_with_input` (a deadline, a byte
budget, no child left behind, refused under the Core lock). The address, the key header, the
form fields and the body go in the configuration `curl` reads on standard input, so a key is never
on a command line.

- `openai-images`: `POST {endpoint}/images/edits`, a multipart form with `image`, `model`,
  `prompt` and `size`; the answer is the picture as base64, or an https address that is fetched
  with no key.
- `comfyui`: `POST /upload/image`, `POST /prompt` with the entry's graph and this edit's values
  (picture, instruction, seed, width, height), `GET /history/ID` until it is complete, and
  `GET /view` for the saved picture.

A provider whose tool is the harness's own image tool needs no adapter: the agent makes the frame
with that tool and runs `check` on the file.

### The person's key

A key is kept per provider in `<data>/image-keys/<id>`, an owner-only file in an owner-only folder
(`soyeht_platform::fs`). `soyeht avatar key set PROVIDER` reads it from standard input and
`key clear` forgets it; `providers` says only whether one is stored. The key is never printed,
never logged and never part of an error. This is the build host's vault of ADR 0166 section 2: the
engine does not receive keys from the clients, and a phone that uses an avatar needs none.

## The frame spec

`soyeht-image/data/frames.json`. Each frame has an id (the ids of the avatar package: `closed_MBP`,
`A`, `E`, `I`, `O`, `U`, `FV`, `LDT`, `blink`, `half`, `left`, `right`, `up`, `down`, `upleft`,
`soft`, and the resting mouths `smile` and `hm`), a group and the change it asks for. A group holds
the prompt template, what must stay the same, and the least and most of the picture the change may
cover. The ids are tied to `soyeht-avatar` by a test of `soyeht-cli`.

`soyeht avatar check` cuts the picture into 16-pixel squares and counts a square as changed when
a tenth of its pixels moved by more than 24 (of 255), so the faint noise of a model that redraws
the whole picture is not a change. It reports the changed share, the box around the changed
squares and, when the agent gives `--region`, the squares outside it, and names the problems:
`size_differs`, `unchanged`, `changed_too_much`, `outside_region`. These measures and the
thresholds of each group are data and **have not been tuned on real outputs yet** (no image model
was available when this was written); the first runs on the GPU host set them. The check cannot see
identity, a head that tilted by a little, or whether a mouth looks like its sound: the agent looks
at the frame, as the skill says. Face landmarks come with the build (slice 9b).

## The commands

```
soyeht avatar providers                       the catalogue, with whether a key is stored
soyeht avatar frames                          the 18 frames
soyeht avatar frame --provider ID --frame FRAME --source PICTURE.png --out OUT.png \
    [--endpoint URL] [--model NAME] [--seed N] [--extra TEXT] [--timeout SECONDS]
soyeht avatar check --frame FRAME --source PICTURE.png --image OUT.png [--region X,Y,W,H]
soyeht avatar key set PROVIDER | soyeht avatar key clear PROVIDER
```

All of them print JSON. They work with no engine running.

## The skill

`engine/soyeht-skills/skills/avatar-builder/SKILL.md` tells an agent how to produce the set: what
to ask the person first, how to use the two commands, what to look for in a frame, when to redo
one and when to stop. It holds no list of frames (the command prints it). The build job of
slices 1 and 2 puts it where the avatar's helper pane loads it; `soyeht_skills::bundled` carries
the text.

## The preset pack

Not generated yet: the GPU host's driver was not loaded when slice 9a was written. When it is, for
each of the 6 to 8 presets:

1. Make the portrait with Qwen-Image 2.1 on the project's ComfyUI: one synthetic person (no real
   person), photo-like, one face looking at the camera, centred and large, lips closed, eyes open,
   1024 by 1024 pixels, a plain background. Keep it as `source.png` in a folder of its own.
2. In a Soyeht pane, ask an agent: "Use the avatar-builder skill with the picture `PRESET/source.png`,
   the provider `qwen-image-2.1-comfyui` and the server at `http://HOST:8188`, and write the frames
   to `PRESET/frames`." The agent runs `soyeht avatar frame` and `check` for each frame. The
   lab measured about seven tries per frame and 14 seconds per try on an RTX 4090, so about 35 GPU
   minutes per preset.
3. When `soyeht avatar build` exists (slice 9b), build the package from `PRESET/frames`.
4. Collect the packages as the downloadable pack, with its notice.

The first run also measures what this document says is not measured: the thresholds of
`frames.json`, and the cost per image of a cloud entry.
