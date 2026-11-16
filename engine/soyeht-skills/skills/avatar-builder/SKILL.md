---
name: avatar-builder
description: Build the frames of a Soyeht avatar from one face picture. Make each edited frame with the image provider the person chose, check it, look at it, and redo the ones that are wrong. Use when the person asks to build, rebuild or fix an avatar, or when you are given a picture and an image provider to build an avatar from.
---

# Build the frames of an avatar

An avatar is one picture of a face plus a set of edited copies of it: eight mouth shapes, a
blink, seven eye frames and two resting mouths. The app moves the mouth continuously between the
mouth shapes while the avatar speaks, so every frame must be the same picture with exactly one
feature changed. A frame where the head moved, the face looks like another person or the lighting
changed breaks the animation. Your job is to produce a clean set and to say honestly which frames
you could not get.

You do the looking and the deciding. Two commands do the plumbing:

- `soyeht avatar frame` makes one frame with an image provider.
- `soyeht avatar check` measures a frame against the source and names what is wrong with it.

## Before the first frame

1. **The picture.** One PNG of one face, looking at the camera, the face centred and large, with
   the lips closed and relaxed and the eyes open. A square of 1024 by 1024 pixels is best. The
   provider's `sizeMultiple` (see `soyeht avatar providers`) says what both sides must be a
   multiple of. If the picture is not like that, tell the person and prepare a cropped copy with
   the tools you have; never alter the original.
2. **The provider.** `soyeht avatar providers` lists the image catalogue: each entry says whether
   it runs on a computer of the person's own (`local`) or someone else's (`cloud`), whether it needs
   the person's own key (`needsKey`) and for which addresses one is stored (`keyHosts`), what it
   receives (`sends`), whether it takes a model name (`takesModel`) and a seed (`takesSeed`), and its
   license. Use the provider the person named; if they named none, show the list and ask.
   **Ask the person for a cloud provider's address and model name. Never guess them, and never
   take them from an earlier conversation or from another provider.** A local provider starts
   from its own address; give `--endpoint` when the person named another computer of theirs.
3. **The key.** A cloud provider needs the person's own key, stored for that provider and that
   address (`keyHosts` lists the addresses). If there is none for the address the person gave, ask
   them to run `soyeht avatar key set PROVIDER --endpoint ADDRESS` in a terminal pane of this
   workspace in Soyeht (keys belong to the Soyeht profile, so another terminal app stores them
   elsewhere), and to type the key and press Enter. Never ask for the key in the chat, never read
   the key file and never put a key in a command.
4. **Say what will happen.** A cloud provider receives the picture and the instructions and may
   charge for each image. A full set is 18 frames, and getting each right took about seven tries
   when it was measured, so up to about 125 images. Tell the person before you start, and stop when
   they say stop.

## The set

`soyeht avatar frames` prints every frame with its id, its group (`mouth`, `blink`, `eyes`,
`resting`) and the change it asks for. That list is the truth; make every frame in it. Work in a
folder next to the picture and name each file after its frame.

## For each frame

1. Make it:

   ```
   soyeht avatar frame --provider PROVIDER --frame FRAME --source PICTURE.png --out WORK/FRAME.png
   ```

   Add `--endpoint` and `--model` as the person gave them. A local provider can take many minutes
   the first time it starts. Give the command a long timeout or run it in the background, and do
   not start a second one against the same provider.
2. Check it:

   ```
   soyeht avatar check --frame FRAME --source PICTURE.png --image WORK/FRAME.png
   ```

   `size_differs` is a hard failure: the frame cannot be used. So is `outside_region` once the
   region comes from frames you accepted (below). The rest (`unchanged`, `changed_too_much` and the
   numbers) is advice: the thresholds have not been tuned on real pictures yet, so a frame that
   looks right to you and fails one of those is still accepted, and a frame that passes but looks
   wrong is not. `changedBox` is where the change is.
3. Look at the frame next to the picture, whatever `check` said. `check` cannot see a face that
   became another person. Accept a frame only when: the person is recognisably the same; the head,
   hair, glasses, lighting, framing and background did not move; only the named feature changed;
   and the feature looks like what the frame asks for (a mouth shape looks like that sound, an
   eye frame looks where it should, a blink has the eyes closed).
4. If it is wrong, make it again. A provider that `takesSeed` gives another picture for another
   `--seed`; one that does not gives the same kind of picture again, so change the instruction
   instead. When the same mistake repeats, add a short correction with `--extra` (for example
   `--extra "the teeth are not visible"` or `--extra "the head stays exactly where it is"`). After
   about eight tries on one frame, go on to the next and come back at the end.

Do the mouth frames first, the widest-open ones before the closed one (`A`, `O` and `E` before
`closed_MBP`). Once several are accepted, give `--region X,Y,WIDTH,HEIGHT` to the next ones: the
box that holds every accepted frame's `changedBox`, widened by about 20 percent. Do not take the
region from the closed mouth alone: it is the smallest change, and the open ones would fail
against it. Do the same for the eye frames once some of them are accepted.

Do not edit a frame by hand, paste parts of one frame into another, or accept a frame to finish
sooner. A frame you could not get is better reported than faked.

## When you are done

Tell the person which frames are accepted, which are not and why, and where the files are. If
`soyeht --help` lists `avatar build`, run it on the folder as the last step; if it does not, leave
the accepted frames where they are and say that the packaging step is not in this version.
