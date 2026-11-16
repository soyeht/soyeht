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
alphas 0, 128 and 255, mixes of clear and opaque texels, and two pairs that must agree to under a
255th: a layer of the weight 0.0002, and a drawing in front of a clear texel that passes the
layer threshold). The layers are first **mixed by their weights**, each over the mix before it
(`mix = texel * a + mix * (1 - a)`, `a` the layer's `Layer::alpha`, from a clear mix; the first
layer's alpha is 1), the texel's own opacity being part of the colour that is mixed, so a clear
texel of a heavy drawing clears the mix in proportion to its weight and a drawing that comes in
at a tiny weight changes the mix by a tiny amount, whatever the opacity of the texels. The mix is
then laid over what was below the part once, by the coverage, which is the part's opacity times
the feather of its corners (`mix * c + under * (1 - c * mix alpha)`). Laying each layer over the
picture with the feather, or each layer over the one before with `1 - a * texel alpha`, would count
the feather once for each drawing in play and let a clear texel leave an earlier drawing standing,
and the picture would step when a layer came in.

Half way between closed or rest and a vowel the inner mouth is drawn at partial opacity, where the
lab drew it opaque. This look is still to be judged by the owner on a real face.

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
part, the work of the mouth (below), the texture power (at most 2), a drawing with no picture in a
part that is always drawn, path names (plain ASCII, no reserved Windows name, no two names that differ only in case, no
bidirectional override) and every number of the acting constants. A package is a file from a
stranger: a constant that would make the pose absurd or not a number is refused.

## The work of the mouth

What a renderer fills for a frame is bounded by a load rule, `blend_work` (kind `tooLarge`, at
`/mouth/parts`). For each triangle of each of the three parts, take its three corners in all
nine drawings of the part (27 points) and the box that holds them, `(max x - min x) * (max y - min
y)`; add the areas of those boxes over every triangle of the three parts; divide by `canvas width
* canvas height`. The package is refused when the result is over 1 (`MAX_DRAW_AREA`, a canvas).
Every frame draws a triangle at a convex blend of its corners, which lies inside that box, so no
drawing of a frame is larger than the box, and two drawings that have no area but blend into a
large triangle (their corners on two different lines) are counted as what they can make. The
lab's mouth (160, 60 and 120 triangles) adds up to 0.23 of its canvas. A port computes it with
the corners as the document gives them, in double precision, comparing with `>`; the cases at
0.99 and 1.01 canvases leave the edge unexercised.

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

The crate `soyeht-avatar-raster` of `apps/desktop` draws a package, with no window; every rule is
`soyeht-avatar`'s. It is optimised in debug builds too (`[profile.dev.package]` of the desktop
workspace), since a frame in an unoptimised build is fifty times slower. `load_folder` reads a
package folder as this document asks a caller to. The names the document lists are checked against
`plain_png_path` before anything is opened; every file is then opened once, through
`soyeht_platform::fs::read_regular_under` (no link in the name or in a folder of the path, none
followed, a regular file only, never waiting on a pipe, at most the limit of the load rules), and
the bytes that were measured for the rules are the bytes that are decoded. Then `Package::load`,
and only then the PNGs are decoded.

A picture is drawn at the size it has on screen (`Rasteriser::render(frame, width)`, never more
than the canvas), so that the work is by the output and not by the canvas: the base, the eye
frames and the atlas of the mouth are reduced once to that size and the frame is composed there.
**The filter** is an area average (a box filter, separable, with the fractional share of the
texels the output pixel covers as the weights) on the premultiplied channels; a size that is not
smaller is the picture as it is. A mouth mesh is scaled by the same factors, so a textured triangle
reads the reduced atlas at about one texel for each pixel. A frame is then:

1. the base picture, with the eye frame laid over it, source-over, in the rectangle that covers the
   pixels the frame touches at this size. **Which frame is a rule of the crate**, `eye_frame(eye)`:
   the blink when the lid is closed, `half` when it is half closed, else the frame of the gaze
   when the eyes look away from the centre, else `soft` while listening, else none. The vectors
   `eyeFrames` of `avatar-mouth.json` pin it for every gaze, lid and soft combination;
2. the mouth: each part in the order `MouthModel::in_drawing_order` gives (skin, lips, inner,
   whatever order the package lists them in; vectors `drawOrder` of `avatar-package.json`), at
   the vertices `MeshPart::blend` gives. A part is drawn once: each pixel reads every drawing
   `MeshPart::layers` names, the drawings are mixed, and the mix is laid over what is below at
   `MeshPart::opacity` times the feather (`composite`). A triangle is filled with exact integer
   edge tests (corners snapped to 1/16 pixel) and the **top-left rule** (with y down, a pixel
   exactly on a top edge, which runs to the right, or on a left edge, which runs up, belongs to the
   triangle; on the other edges it belongs to the neighbour), so two triangles that share an edge
   never draw a pixel twice and never leave one out;
3. the pose: `Warp::source_point` is evaluated on a grid and each output pixel reads the picture,
   bilinearly, at the position interpolated across its cell. The cell is 4 output pixels, and 2
   where the cell lies on a place the pose moves that is small (under 32 pixels across at this
   size: a brow, a cheek).

**The stage** (`AvatarStage` of `soyeht-desktop`) asks a `Driver` once a frame for the state and
the audio that plays, steps `Runtime` by the time that really passed (at most 0.1 s), and has the
picture drawn on the background executor; only the finished picture is assigned, and painted, on
the main thread. The pace is `Pacer`'s: a picture every 1/30 s, every 1/60 s while the avatar
speaks, at most 512 pixels across until a frame is measured on Windows and at least 64; a frame
that cost more than three fifths of its interval in the drawing thread's processor time
(`soyeht_platform::cpu::thread_time`) makes the next pictures a quarter narrower, and three
seconds of frames under a third of the interval make them a third wider again. A window that is
not painted for half a second is taken as hidden and its timer ends. The voice, the
configuration screens and the pane are later slices.

**Frame cost** (Linux, one core, an Intel Xeon E5-2680 v4 at 2.4 GHz on a shared host, so a slow
core; a canvas of 512 pixels drawn at 512 by 512, a mouth mesh of 320 triangles in three parts, a
person speaking; mean of 600 frames): **11.7 ms** in a release build (median 11.7, 95th percentile 12.5) and **13.5 ms** in a debug build,
with the crates of the avatar optimised (median 13.4, 95th percentile 14.7). Of a frame, about 2 ms composes the picture (the eyes and
the mouth), 4 ms evaluates the rule of the pose on the grid and 6 ms reads the picture through it
(the host is shared, and the split varies by a few milliseconds). Drawn 256 wide it is 3.5 ms. A
canvas of 1024 drawn 512 wide is 12.5 ms and drawn 1024 wide 60 ms. At the cap of 512 pixels it is
under a 60 Hz frame (16.7 ms) on this core but over three fifths of it, so while the avatar speaks
the pacer narrows the picture, which is what it is for. **Windows is still to be measured**; how much faster a laptop core is
is not yet measured. The numbers come from `cargo test --release -p soyeht-avatar-raster
tests::budget -- --ignored --nocapture` (`frame_budget_at_the_usual_size`, `frame_cost_by_size`,
`frame_cost_by_step`). The GPU path the ADR leaves for later (wgpu through GPUI) is what to build
if a Windows laptop does not reach the budget; the rasteriser stays as the reference the vectors
and the tests hold.

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

## The Swift runtime and rasteriser (slice 4)

`apps/shared` (`SoyehtClient`) holds the second implementation, in the `Avatar*` files: the package
reader and validator (`AvatarPackage`, `AvatarPackageValidator`, with the same limits, the same
order of rules and the same failure kinds), the mouth (`AvatarParams`, `AvatarMorphWeights`,
`AvatarTrack`, `AvatarClip`, `AvatarMouth`), the aligner (`AvatarLetters`, `AvatarAligner`), the
acting layer (`AvatarActing`, with the bounds of `AvatarActingBounds`), the runtime
(`AvatarRuntime`) and the warp (`AvatarWarp.sourcePoint`). It passes every case of both vector files
(`AvatarMouthVectorTests`, `AvatarPackageVectorTests`), and a test fails when either file gains a
group of cases that no Swift test runs.

**The reader is looser than the Rust one in what the vectors cannot say.** A package field that is
present with `null`, or with another type, is malformed, as in the Rust reader; a field that is
absent keeps its default. The document must open with `{` after blanks and holds no NUL byte (so a
byte-order mark and UTF-16 are malformed), and paths are compared by their UTF-8 bytes, not by
Swift's canonical equivalence. Foundation's decoder still accepts what `serde_json` does not: a
number written `512.0` or `1e2` for an integer, a repeated key (Foundation takes one of the
values, the Rust reader refuses the document), and a trailing comma. Closing that takes a strict pre-scan and raw-document vector cases, which are a follow-up.

The rasteriser is software, on one core, in premultiplied RGBA bytes, and has no GPU path yet.
The per-pixel code (`AvatarRasterizer`, `AvatarBitmap`, the loop of the warp) is its own target of
the `apps/shared` package, `SoyehtAvatarRaster`, compiled optimised in a debug build too (an unsafe
flag, allowed because the package is consumed by path); it shares its types with `SoyehtClient`
by `package` access. `AvatarRenderer`, in `SoyehtClient`, drives it. A frame is:

1. the base picture, scaled once at load with CoreGraphics' high-quality interpolation (a smooth
   filter, not an exact area average: a canvas of 1024 pixels drawn at 512 is not point-sampled)
   to the size of the output, at most 512 pixels on the longer side and never larger than the
   canvas;
2. the eye frame in place of the pixels it covers. Each frame is laid over the base picture at the
   size of the canvas, the picture is scaled, and the rectangle (rounded outward) is cut out, so
   the frame lands where the scaled picture has the eye at any scale. Which frame is drawn, and the
   order of the mouth's parts below, are rules every renderer must share; the vector cases for
   them arrive with slice 5 (`eyeFrames` in `avatar-mouth.json`, `drawOrder` in
   `avatar-package.json`). Until then Swift draws the blink when the lid is closed, `half` when it
   is half closed, else the frame of the gaze when the eyes are off centre, else `soft`, else
   none; and the parts as `skin`, `lips`, `inner`;
3. the mouth: for each part, the vertices are the weighted sum of the nine drawings' contours, and
   each textured drawing whose weight is over 0.0001 is drawn over the previous ones through its
   own texture coordinates, with the opacity the mesh gives it (`AvatarMeshPart.layers`), so the
   layers are the exact weighted average. A triangle is filled by barycentric coordinates with the
   top-left rule, so two triangles that share an edge never draw a pixel twice, and a texel's own
   alpha is what the picture under it gives up. A part's feather (the vertices' opacity) is applied
   once, over the picture as it was before the part, and the layers mix over that, so an edge does
   not change when a layer comes in. A triangle costs the pixels it covers (each row is cut to the
   span the edges leave), and a drawing's triangles may cover at most one canvas: the renderer
   refuses a package that exceeds it, once, when it is made (`tooLarge`), never frame by frame,
   because a frame that left the mouth out would put the base picture's own mouth in its place.
   That rule is the Swift reader's own until the shared load rule of the Rust validator and
   `avatar-package.json` replaces it. Only the pixels the eye frame and the mouth touched are
   restored from the base picture the next frame;
4. the warp: `AvatarWarp.sourcePoint` is evaluated on a grid of 8 output pixels, and each output
   pixel reads the picture, bilinearly, at the position interpolated across its cell.

There is no code path that picks a mouth drawing. A test renders the mouth from every drawing to
every other in steps and holds the largest change of any pixel in a step to under five times the even
step (the worst pair is 4.5).

**The stage** (`AvatarStageModel`, `AvatarStageView`; the Mac's `MacAvatarStage` and the iOS app's
`PhoneAvatarStage` give it the flat surface of the palette). An actor (`AvatarStageEngine`) owns
the runtime and the renderer; the package is read, once, and the first frame drawn off the main
actor, and the main actor only assigns the finished picture. Frames are paced by completion: the
next one starts when the last has been drawn, no sooner than 1/30 s after it started (1/60 s while
speaking). The loop runs while at least one view of the model is on screen, in a scene that is not
in the background, and (on the Mac) in a window that is not covered or minimised; there is one loop
however many views show the model. With Reduce Motion on, one still picture is drawn per state and
nothing is drawn between changes of state. A package that did not load leaves a failure the views
show by its kind (`failureText`, no path) and the log records.

The picture starts at the size the renderer made (at most 512). A frame that costs, in thread CPU
time, more than the time it has, six times in a row, makes the picture a quarter smaller (down to
96 pixels); frames that cost under half of it for ten seconds make it a third larger again, up to
where it started, and the wait doubles each time a regrown picture has to shrink again within it.
Each change is logged. The full-size pictures stay in the engine for this. The voice, the
configuration and the pane are later slices; until then the mouth holds the expression of the
state.

**Frame cost** (Mac Studio, one core, thread CPU time, the host loaded by other work, a 1024-pixel
canvas drawn at 512 by 512, a mouth mesh of about 4800 triangles in three parts): about 5.7 ms
median (p95 9 ms) in a release build for the runtime step and the picture, and the same with the
`CGImage` the view draws; all of it runs off the main actor. A debug build, with the per-pixel
target optimised, is about 19 ms. The cost is that of the output, not of the canvas. The desktop's
rasteriser (slice 5) measures its own.

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
