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

**There is no code path that swaps or cross-fades mouth drawings, for the mouth or for a resting
expression.** That is the owner's rule (2026-11-05: the first lip-sync, which swapped and
cross-faded drawings, scored 1 out of 10).

The lab's listening smile and thinking "hm" were two more mouth pictures, swapped in with a short
fade. They are not in the package. A resting expression is a target of the continuous mouth: the
acting constants hold `expression.listening` and `expression.thinking` (five parameters each), the
runtime gives them to the mouth while nothing is said, and the mouth morphs there and back like
everywhere else. The defaults are a slight spread (a smile) and a pressed, slightly pursed mouth.

The acceptance of any change to the mouth is the lab's checker limits, which the crate's tests
port to geometry (the largest one-frame change at most 0.65 of the closed to A distance and 0.70
inside a closure, M/B/P closed at least 90 %, silences closed, at most 7 reversals per second,
opening against the voice at least 0.55) and a look at a contact sheet across the whole clip by a
person: the rejected H1 mouth passed the checker. The lab's pixel checks (the seam of the morph
boundary, a smeared corner, the ghost frames of a dissolve) belong to the renderers. What holds the
"one mouth" rule in this crate is a property test: over a grid of the parameter space the weights
are never negative and sum to 1, and the layers of the picture give each drawing exactly its
weight.

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
text, or a track with more sounds than speech can hold, which means the text does not belong to the
audio), the runtime follows the level of the voice instead: a voice never plays over a shut mouth.

## The package

One JSON document (`avatar.json`) next to the PNG files it lists: the base picture (the size of the
canvas), the mouth model (nine shapes per part with one triangulation, in the parts `skin`, `lips`
and `inner`, and the atlas of the nine mouth crops), the eight eye frames with their rectangles, the
`warp` and optional acting constants.

`warp` is where the pose applies on this face, in canvas pixels: the neck pivot, the zoom centre,
the head, face, brow, jaw and cheek regions (a centre, two radii and the share of them that is full
strength), where the head's influence fades over the neck, the brow tilt span, and the chest's rows,
spread and rise. `Warp::source_point(pose, x, y)` is the one rule that uses them (a port of the
lab's shader, with the numbers of one face replaced by the package's), so the renderers agree and a
pose can be applied to another face.

The caller reads the folder; `Package::load` checks the document against the files it lists and
returns a failure kind (stable, camelCase) and a JSON pointer, never a value. The limits are in
`package/limits.rs` and `package/acting_bounds.rs`, each with its reason, and are pinned by
`contracts/vectors/avatar-package.json` with a case at the limit (it loads) and one past it (it
fails): the document (4 MiB, checked before it is parsed), the canvas and pictures (2048 on a side),
the files (64, 4 MiB each, 8 MiB together, 8 Mpx decoded together), the vertices and triangles of a
part, path names (plain ASCII, no reserved Windows name, no two names that differ only in case, no
bidirectional override) and every number of the acting constants. A package is a file from a
stranger: a constant that would make the pose absurd or not a number is refused.

## The acting layer and the canvas

The acting constants are written for a picture 1024 pixels tall (`REFERENCE_SIZE`). `Acting::new`
takes the real canvas height and scales every length it returns (`head_*`, `face_*`, `brow_*`,
`jaw`, `cheek`) by it over 1024; angles and the unitless `breath` and `zoom` do not scale. The
runtime feeds the acting layer the chin drop of the lab, `opening * 4.6 * (0.8 + 0.2 * (1 -
closure))`, after the mouth has stepped, and raises the accent of each word the audio reaches when
the clip carries its word starts (`Clip::with_words`, `Alignment::words`).

## Inputs from the host

A host can hand over anything. A step whose `dt` is not a finite number is skipped by the mouth and
is a step of no time for the acting layer; a mouth target with a parameter that is not a number is
silence; a level that is not a number, or is `-inf` (a silent buffer is `log10(0)`), is the floor of
-120 dB; an envelope with a hop outside 1 ms to 1 s, or no levels, has no voice; a phoneme with no
finite time is not heard. None of these leaves a state that is not a number.

## The vector files

`contracts/vectors/avatar-mouth.json` (weights, the target track, the mouth's motion at each 1/60
s, the aligner, the acting layer, the runtime, the warp) and `contracts/vectors/avatar-package.json`
(the load rules). Other implementations agree with the numbers within the file's `tolerance`. Every
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
