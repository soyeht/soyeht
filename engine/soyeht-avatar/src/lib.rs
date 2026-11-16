#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! The avatar of a workspace: a pure crate with no IO and no async runtime (ADR 0166, slice 3).
//!
//! - `package`: the avatar package format, its loader and its validator.
//! - the **continuous mouth** (H0, the validated one): five parameters (opening, width, rounding,
//!   closure, lip-teeth contact) that follow phoneme targets with eased motion and coarticulation
//!   (`track`, `mouth`), and the morph weights that blend the lip contours of nine drawings
//!   (`params`, `mesh`). The mouth is parameters and geometry. It is never a step between pictures
//!   and never a cross-fade of two mouths: nothing in this crate picks a drawing.
//! - `aligner`: the text of a reply and the envelope of the audio that actually plays become a
//!   timed phoneme track, for a TTS that gives no timings.
//! - `acting`: the four states of the avatar and the small motions of a living head, with a seeded
//!   generator.
//! - `runtime`: one avatar's performance, stepped by the audio clock.
//!
//! The Swift and the Rust renderers pass the same vectors (`contracts/vectors/avatar-mouth.json`
//! and `avatar-package.json`). The reference is the avatar lab's runtime, whose `rig/` is
//! Apache-2.0 like this repository: `mouth.js`, `core.js` (the blend) and `sabio-acting.js` are
//! ported; the authoring tools are not (they are Python and stay in the lab until the build
//! command of slice 9b).

mod acting;
mod acting_tune;
mod aligner;
mod clip;
mod envelope;
mod eye_frame;
mod mesh;
mod mouth;
mod package;
mod params;
mod phoneme;
mod rng;
mod runtime;
mod smooth;
mod track;
mod tune;
mod warp;

#[cfg(test)]
mod tests;

pub use acting::{
    Acting, ActingFrame, ActingInput, Eye, Gaze, Lid, Pose, REFERENCE_HEAD, State, face_scale,
};
pub use acting_tune::{
    ActingTune, BlinkTune, BreathTune, BrowTune, ExpressionTune, GazeTune, HeadTune, StressTune,
};
pub use aligner::{Alignment, Language, align};
pub use clip::{Clip, WordStart, energy_target};
pub use envelope::{Envelope, HOP};
pub use eye_frame::eye_frame;
pub use mesh::Layer;
pub use mouth::Mouth;
pub use package::{
    AssetInfo, Atlas, Chest, EYE_IDS, ErrorKind, FORMAT, FileDecl, MAX_DOCUMENT_BYTES,
    MAX_FILE_BYTES, MAX_FILES, MeshPart, MouthModel, Package, PackageError, PartKind, Patch,
    Region, ShapeMesh, Size, VERSION, Warp, png_size,
};
pub use params::{MorphWeights, Params, Shape, weights_from_params};
pub use phoneme::{Kind, Phoneme};
pub use rng::Rng;
pub use runtime::{Audio, Frame, Runtime};
pub use track::{Level, Target, build_track, target_at};
pub use tune::Tune;
