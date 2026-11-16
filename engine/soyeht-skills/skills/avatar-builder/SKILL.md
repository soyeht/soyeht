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
   the lips closed and relaxed and the eyes open. A square of 1024 by 1024 pixels is best. Both sides
   must be a multiple of 16 for a local ComfyUI provider. If the picture is not like that, tell the
   person and prepare a cropped copy with the tools you have; never alter the original.
2. **The provider.** `soyeht avatar providers` lists the image catalogue: each entry says whether
   it runs on a computer of the person's own (`local`) or someone else's (`cloud`), the model it
   uses, whether it needs the person's own key (`needsKey`, `keySet`), what it receives (`sends`)
   and its license. Use the provider the person named. If they named none, show the list and ask.
   Give `--endpoint` and `--model` when the person named their own address or model.
3. **The key.** A cloud provider needs the person's own key. If `keySet` is false, ask the person to
   run `soyeht avatar key set PROVIDER` in their own terminal and type the key on standard input.
   Never ask for the key in the chat, never read the key file and never put a key in a command.
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
   soyeht avatar frame --provider PROVIDER --frame FRAME --source PICTURE.png --out WORK/FRAME.png --seed 1
   ```

   A local provider can take many minutes the first time it starts. Give the command a long
   timeout or run it in the background, and do not start a second one against the same provider.
2. Check it:

   ```
   soyeht avatar check --frame FRAME --source PICTURE.png --image WORK/FRAME.png
   ```

   `ok` is false when the size differs, when almost nothing changed, when too much changed, or
   when something changed outside the region you gave. `changedBox` is where the change is.
3. Look at the frame next to the picture, whatever `check` said. `check` cannot see a face that
   became another person. Accept a frame only when: the person is recognisably the same; the head,
   hair, glasses, lighting, framing and background did not move; only the named feature changed;
   and the feature looks like what the frame asks for (a mouth shape looks like that sound, an
   eye frame looks where it should, a blink has the eyes closed).
4. If it is wrong, make it again with another `--seed`. When the same mistake repeats, add a short
   correction with `--extra` (for example `--extra "the teeth are not visible"` or `--extra "the
   head stays exactly where it is"`). After about eight tries on one frame, go on to the next and
   come back at the end.

Do the mouth frames first. When the first mouth frame is accepted, take its `changedBox`, widen it
by about 20 percent, and give it as `--region X,Y,WIDTH,HEIGHT` to every other mouth frame, so a
frame that changes anything outside the mouth fails `check`. Do the same for the eye frames once
the first of them is accepted.

Do not edit a frame by hand, paste parts of one frame into another, or accept a frame to finish
sooner. A frame you could not get is better reported than faked.

## When you are done

Tell the person which frames are accepted, which are not and why, and where the files are. If
`soyeht --help` lists `avatar build`, run it on the folder as the last step; if it does not, leave
the accepted frames where they are and say that the packaging step is not in this version.
