# The avatar runtime (`soyeht-avatar`)

ADR 0166, slice 3. A pure Rust crate (no IO, no async runtime, no Soyeht dependencies) holding what
every renderer of an avatar must agree on: the package format, the continuous mouth, the
audio-to-mouth aligner and the acting layer. Renderers (the Swift one of `apps/shared`, the
desktop's rasteriser) are later slices; they pass the same vector files as this crate.

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

**There is no code path that swaps or cross-fades mouth drawings.** That is the owner's rule of
2026-11-05 (the first lip-sync, which swapped and cross-faded drawings, scored 1 out of 10).

The acceptance of any change to the mouth is the lab's checker limits, which the crate's tests
port to geometry (ghost frames 0, the largest one-frame change at most 0.65 of the closed to A
distance and 0.70 inside a closure, M/B/P closed at least 90 %, silences closed, at most 7
reversals per second, opening against the voice at least 0.55) and a look at a contact sheet
across the whole clip by a person: the rejected H1 mouth passed the checker.

## The aligner

For a TTS that gives no phoneme timings. The text becomes phonemes by letter rules of our own
(Portuguese of Brazil and a coarser English; the usual phonemizer is GPL and is not used), the
envelope of the audio that really plays gives the voiced span and the silences inside it, and the
words are placed in the voiced span in proportion to their sounds, then pulled to the silences by a
monotone matching, so a word is stretched to fit between the silences that surround it. It uses
the voiced span and the silences only; it does not listen to the vowels.

## The package

One JSON document (`avatar.json`) next to the PNG files it lists: the base picture, the mouth
model (nine shapes per part with one triangulation, in the parts `skin`, `lips` and `inner`, and
the atlas of the nine mouth crops), the eight eye frames and the two resting mouths with their
rectangles, the warp lattice of the head, and optional acting constants. The caller reads the
folder; `Package::load` checks the document against the files it lists (path rules, sizes, the
shapes, one vertex list, triangle indices, texture coordinates inside each shape's cell, frame
rectangles) and returns a failure kind (stable, camelCase) and a JSON pointer, never a value. The
major version is 1; a later minor version may add fields, which an older reader ignores.

## The vector files

`contracts/vectors/avatar-mouth.json` (weights, the target track, the mouth's motion at each 1/60
s, the aligner, the acting layer) and `contracts/vectors/avatar-package.json` (the load rules).
Other implementations agree with the numbers within the file's `tolerance`. The clock of the acting
layer is a sum of 1/60 s steps in a double, and its random numbers are SplitMix64, so the same seed
gives the same blinks and glances.

After a deliberate change of the runtime, rewrite the files with
`SOYEHT_UPDATE_VECTORS=1 cargo test -p soyeht-avatar vectors` and read the diff. A vector that
changes is a mouth that changes.

## What was ported, and from where

The reference is the standalone avatar lab, outside this repository. Its `rig/` is Apache-2.0
(`Copyright 2026 The Soyeht authors`), the license of this repository. Ported: the viseme
anchors, the weights, the phoneme targets, the damped follower and the dynamics of
`rig/runtime/mouth.js`; the blend and its picture layers of `rig/runtime/core.js`; the acting
layer of `static/sabio-acting.js` (with a seeded generator in place of `Math.random`); the limits
of `mouth/check_mouth.py`; and the pt-BR letter rules of `phonemesFromWords`. Not ported: the
authoring tools (`rig/author/*.py`, Python and specific to the lab's art), which wait for the
build command of slice 9b, the H1 mouths and the layered rig. No picture of the lab is copied: the
tests build a small synthetic avatar.

Fidelity was checked once against the lab's own code, not in the gate: the lab's `mouth.js` and
`sabio-acting.js`, run in Node on the same inputs, agree with this crate to the rounding of the
vector files over the mouth's motion (576 frames in three clips) and over the acting layer (4026
frames in three seeds, with the same random numbers).
