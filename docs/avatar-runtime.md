# The avatar runtime (`soyeht-avatar`)

ADR 0166, slice 3. A pure Rust crate (no IO, no async runtime, no Soyeht dependencies) holding what
every renderer of an avatar must agree on: the package format, the continuous mouth, the
audio-to-mouth aligner, the acting layer and the rule that applies a pose to a face. Renderers (the
Swift one of `apps/shared`, the desktop's rasteriser) are later slices; they pass the same vector
files as this crate.

**The package format is unstable.** Its version is `0.1`, a reader accepts that string and no
other, and any change to the format before 1.0 may break an older reader. It will stay 0.x until the
renderers of slices 4 and 5 and the build command of slice 9b have consumed it, because what a
renderer and a builder need is not known until they exist. From 1.0 on, a later minor version only
adds fields.

## The continuous mouth (H0)

A mouth is five parameters, each in 0 to 1: opening, width, rounding, closure (lips pressed, for
M, B and P) and lip-teeth contact (F and V). Phonemes are targets in that space. Every parameter
follows its target with critically damped motion (a faster attack than release) and a slew limit,
so the mouth passes through every in-between state and never changes in one frame.

The nine drawings of a mouth (`rest`, `closed_MBP`, `A`, `E`, `I`, `O`, `U`, `FV`, `LDT`) are
anchors in the same space. The parameters become nine non-negative morph weights that sum to 1
(inverse-distance weights with a compact support, then one more critically damped filter, which
is linear and keeps the sum). The picture is geometry: the weighted sum of the drawings' lip
contours, which share one vertex list, and the aligned weighted average of their textures. Half
way between closed and A the lips are half open with a half-size aperture, never two half-visible
mouths.

How much of a part is drawn is also continuous in the weights. A drawing may have no picture of
its own for a part that hides when empty: the lab's inner mouth has none for the resting and the
closed drawing, and does cover a sliver of the face there. `MeshPart::opacity` of such a part is
the sum of the weights of the drawings that have a picture of it, so the inner mouth comes in as
the mouth opens, and not whole when a weight passes the threshold under which a drawing takes no
part in `MeshPart::layers` (1e-4; the first layer has the opacity 1 whatever its weight). A part
that is always drawn must have a picture for every drawing (the loader refuses one that has
not), since the first drawing with a picture would otherwise replace another in one frame.

A renderer lays a part over the picture **once**: `composite` is the rule, and the `composite`
cases of `avatar-package.json` pin it (one to three layers, coverages 0.3, 0.5 and 1, texel
alphas 0, 128 and 255, and a layer of the weight 0.0002 that changes a feathered pixel by less
than a 255th). Each layer is laid over what the layers before it made of the picture, starting
from what is below the part (`texel * a + picture * (1 - a * texel alpha)`, `a` the layer's
`Layer::alpha`), which mixes the drawings by their weights; the result takes the place of what
was below by the coverage, which is the part's opacity times the feather of its corners. Laying
each layer over the picture with the feather would count the feather once for each drawing in
play, and the edge of the part would step when a layer came in.

**The texture power** (`texturePower`, 1 to 2) sharpens the weights of the pictures, so the
nearest drawing dominates. Between two drawings a share is `w^p / (w^p + (1 - w)^p)`, which moves
`p` times as fast as the weight at the middle: with a power of 8 (the old limit) the share went from
a tenth to nine tenths while the weight moved a fifth, about one frame at the mouth's rate limits,
a swap of the pictures. A power of 2 (the lab's skin has it, and its other parts have 1) moves the
picture at most twice as fast as the weights.

## The aligner

For a TTS that gives no phoneme timings. The text becomes phonemes by letter rules of our own
(Portuguese of Brazil and a coarser English; the usual phonemizer is GPL and is not used), the
envelope of the audio that really plays gives the voiced span and the silences inside it, and the
words are placed in the voiced span in proportion to their sounds, then pulled to the silences by a
monotone matching (over the few boundary and silence pairs that are close enough, so the work grows
with the length of the text), so a word is stretched to fit between the silences that surround it.
It uses the voiced span and the silences only; it does not listen to the vowels.

A digit, a symbol that is said (`%`, `+`, `/`) and a letter of a script the rules do not read
(Japanese, Cyrillic under the English rules) each get a neutral syllable, so a reply full of
numbers has a mouth that moves. When a clip has nothing to say while the audio is loud (an empty
text, or a text with more than 30 sounds a second of the audio it is placed on, which means the text
does not belong to the audio: `Alignment::fits` is then false and its phonemes are empty), the
runtime follows the level of the voice instead: a voice never plays over a shut mouth. The timings
a TTS gives are never judged this way: a clip follows them at any speed, because it is the aligner
that compares a text with audio, not the clip.

## The package

One JSON document (`avatar.json`) next to the PNG files it lists: the base picture (the size of the
canvas), the mouth model (nine shapes per part with one triangulation, in the parts `skin`, `lips`
and `inner`, and the atlas of the nine mouth crops), the eight eye frames with their rectangles, the
`warp` and optional acting constants.

`warp` is where the pose applies on this face, in canvas pixels: the neck pivot, the zoom centre,
the head, face, brow (the left one left of the right one), jaw and cheek regions (a centre, two radii and the share of them that is full
strength), where the head's influence fades over the neck, the brow tilt span, and the chest's rows,
spread and rise. `Warp::source_point(pose, x, y)` is the one rule that uses them (a port of the
lab's shader, with the numbers of one face replaced by the package's), so the renderers agree and a
pose can be applied to another face. A point it returns can lie outside the canvas; the reader
clamps it to the picture when it samples.

The caller reads the folder; `Package::load` checks the document against the files it lists and
returns a failure kind (stable, camelCase) and a JSON pointer, never a value. The limits are in
`package/limits.rs` and `package/acting_bounds.rs`, each with its reason, and are pinned by
`contracts/vectors/avatar-package.json` with a case at the limit (it loads) and one past it (it
fails): the document (4 MiB, checked before it is parsed), the canvas and pictures (2048 on a side),
the files (64, 4 MiB each, 8 MiB together, 8 Mpx decoded together), the vertices and triangles of a
part, the area their triangles cover (for each of the nine drawings, the three parts together,
at most one canvas: what a renderer fills for a frame is bounded; the lab's mouth covers 3%), the
texture power (at most 2), a drawing with no picture in a part that is always drawn, path names (plain ASCII, no reserved Windows name, no two names that differ only in case, no
bidirectional override) and every number of the acting constants. A package is a file from a
stranger: a constant that would make the pose absurd or not a number is refused.

## The acting layer and the size of the face

The acting constants are written for the lab's face: a head whose radii are 370 by 390 pixels
(`REFERENCE_HEAD`), in a picture 1024 pixels tall. `Runtime::new(package, ...)` reads the head's
radii the package carries and `Acting` scales every length it returns (`head_*`, `face_*`,
`brow_*`, `jaw`, `cheek`) by the geometric mean of the ratios (`face_scale`); angles and the
unitless `breath` and `zoom` do not scale. So the motion follows the face, not the canvas: a tall
canvas with a small face gets small motions. The runtime feeds the acting layer the chin drop of
the lab, `opening * 4.6 * (0.8 + 0.2 * (1 - closure))`, after the mouth has stepped, and raises the
accent of each word the audio reaches when the clip carries its word starts (`Clip::with_words`,
`Alignment::words`).

A resting expression (listening, thinking) is held by `Mouth::hold`, which is not speech: it does
not restart the silence rule, so leaving it goes to rest and not through the pressed lips of a
pause.

## The desktop rasteriser (Windows and Linux app)

`apps/desktop`'s `avatar` module draws a package; every rule is the crate's. `load_folder` reads a
package folder as this document asks a caller to: the document (at most `MAX_DOCUMENT_BYTES`),
then the size of each picture it lists (a path that leaves the folder is never opened), then
`Package::load`, and only then it decodes the PNGs. The stage view (`AvatarStage`) asks a `Driver`
once a frame for the state and the audio that plays, steps `Runtime` by the time that really
passed (at most 0.1 s), draws and paints. The voice, the configuration screens and the pane are
later slices.

A frame is drawn in software, on one core, in premultiplied RGBA, and handed to GPUI as a BGRA
image:

1. the base picture, with the eye frame laid over it, source-over. **Which frame is a rule of
   the crate**, `eye_frame(eye)`: the blink when the lid is closed, `half` when it is half
   closed, else the frame of the gaze when the eyes look away from the centre, else `soft` while
   listening, else none. The vectors `eyeFrames` of `avatar-mouth.json` pin it for every
   gaze, lid and soft combination;
2. the mouth: each part in the order `MouthModel::in_drawing_order` gives (skin, lips, inner,
   whatever order the package lists them in; vectors `drawOrder` of `avatar-package.json`), at
   the vertices `MeshPart::blend` gives. A part is drawn once: each pixel reads every drawing
   `MeshPart::layers` names and mixes them, and the picture is laid over what is below at
   `MeshPart::opacity` times the feather (`composite`). A triangle is filled with exact integer edge tests (corners snapped
   to 1/16 pixel) and the top-left rule, so two triangles that share an edge never draw a pixel
   twice;
3. the pose: `Warp::source_point` is evaluated on a grid and each output pixel reads the picture,
   bilinearly, at the position interpolated across its cell. The cell is the smallest place the
   pose moves (a brow, a cheek) over 2.5, between 2 and 8 output pixels, so the number of
   evaluations does not depend on the size of the picture.

The picture is drawn at the size the view has on screen, at most 1024 pixels across and never
more than the canvas.

**Frame cost** (Linux, release build, one core, an Intel Xeon E5-2680 v4 at 2.4 GHz on a shared
host, so a slow core; a canvas of 512 pixels drawn at 512 by 512, a mouth mesh of 1280 triangles
in three parts, a person speaking; mean of 600 frames): **12.6 ms** (median 12.5, 95th percentile
13.4). Of it, about 3 ms composes the picture (the eyes and the mouth), 4 ms evaluates the rule of
the pose on the grid and 6 ms reads the picture through it (the host is shared, and the split
varies by a few milliseconds from run to run). Drawn 256 wide it is 5.2 ms. A canvas of 1024 drawn
512 wide is 18 ms and drawn 1024 wide 50 ms, because the composing is by the canvas. **Windows is still to be measured**; the ADR's frame budget there (one 60 Hz frame,
16.7 ms, for the avatar alone, with half of it as the aim) is met on this core and the aim is
missed by a little; how much faster a laptop core is is not yet measured. The numbers come from
`cargo test --release -p soyeht-desktop avatar::tests::budget -- --ignored --nocapture`
(`frame_budget_at_512`, `frame_cost_by_size`, `frame_cost_by_step`). The GPU path the ADR leaves
for later (wgpu through GPUI) is what to build if a Windows laptop does not reach the aim; the
rasteriser stays as the reference the vectors and the tests hold.

## Inputs from the host

A host can hand over anything. A step whose `dt` is not a finite number is skipped by the mouth and
is a step of no time for the acting layer; a mouth target with a parameter that is not a number is
silence; a level that is not a number, or is `-inf` (a silent buffer is `log10(0)`), is the floor of
-120 dB; an envelope with a hop outside 1 ms to 1 s, or no levels, has no voice; a phoneme with no
finite time is not heard. None of these leaves a state that is not a number.

## The vector files

`contracts/vectors/avatar-mouth.json` (weights, the target track, the mouth's motion at each 1/60
s, the aligner, the acting layer, the runtime, the warp) and `contracts/vectors/avatar-package.json`
(the load rules, and the blend and picture layers of the mouth mesh). Other implementations agree with the numbers within the file's `tolerance`. Every
step is `dt = 1.0 / 60.0` as an IEEE double and the time of frame `i` is the double `i / 60.0`; the
acting layer's clock is the sum of the `dt` in seconds as a double, and its random numbers are
SplitMix64, so the same seed gives the same blinks and glances. A timer that lands exactly on a
frame boundary depends on that arithmetic: a port that keeps its clock in milliseconds can differ
by a frame there.

After a deliberate change of the runtime, rewrite the files with
`SOYEHT_UPDATE_VECTORS=1 cargo test -p soyeht-avatar vectors` and read the diff. A vector that
changes is a mouth that changes.

## Known limits

- The aligner was measured by the review of this crate on four real TTS clips of the lab: with the
  aligner's timings the lips closed on 13 of the 14 M, B and P sounds. Real speech has few
  silences, so the aligner behaves close to a proportional allocation of the words over the voiced
  span; the matching only helps where the audio has real pauses. A trained model of the vowels
  would do better and is not in the launch.
- The per-frame change limit cannot hold at low frame rates: the largest change is 0.72 at 15
  frames a second, over the lab's 0.65, because the same motion is spread over fewer frames. Below
  about 20 frames a second the mouth is correct but coarse. The tests hold the limit at 24, 30 and
  60.
- The voice of the aligner tests is synthetic and tidy. The English letter rules are coarse.
- A checker pass is not evidence: the look across a whole clip, on a photo-like face, is the
  owner's.

## What was ported, and from where

The reference is the standalone avatar lab, outside this repository. Its `rig/` is Apache-2.0
(`Copyright 2026 The Soyeht authors`), the license of this repository. Ported: the viseme
anchors, the weights, the phoneme targets, the damped follower and the dynamics of
`rig/runtime/mouth.js`; the blend and its picture layers of `rig/runtime/core.js`; the acting
layer of `static/sabio-acting.js` (with a seeded generator in place of `Math.random`); the shader
of `static/sabio-warp.js`; the limits of `mouth/check_mouth.py`; and the pt-BR letter rules of
`phonemesFromWords`. Not ported: the authoring tools (`rig/author/*.py`, Python and specific to the
lab's art), which wait for the build command of slice 9b, the H1 mouths, the layered rig, and the
lab's pictures of a resting smile and "hm". No picture of the lab is copied: the tests build a
small synthetic avatar.

Fidelity was checked once against the lab's own code, not in the gate: the lab's `mouth.js` and
`sabio-acting.js`, run in Node on the same inputs, agree with this crate to the rounding of the
vector files over the mouth's motion (576 frames in three clips) and over the acting layer (4026
frames in three seeds, with the same random numbers). The later changes of this crate (the
sanitising of inputs, the linear-time track and matching, the canvas scale) leave the vectors of the
mouth, the aligner and the acting layer unchanged.
