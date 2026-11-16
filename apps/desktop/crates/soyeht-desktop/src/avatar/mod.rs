//! The avatar of a workspace on the Windows and Linux app (ADR 0166, slice 5).
//!
//! Every rule belongs to the crate `soyeht-avatar`: the package format and its bounds, the
//! continuous mouth, the acting layer, and the way a pose moves a face. This module only draws:
//!
//! - `art`: a package folder, validated and decoded into an `Avatar`;
//! - `raster`: the software rasteriser, one core, from the runtime's frame to BGRA pixels;
//! - `view`: the stage, a GPUI view that steps the runtime from a `Driver` each frame and paints
//!   the picture.
//!
//! The mouth is never a choice between pictures: the rasteriser draws the mesh at the geometry
//! the morph weights blend, so exactly one mouth is on screen in every frame.

mod art;
#[cfg(feature = "dev-tools")]
mod demo;
mod pixmap;
mod raster;
mod resample;
#[cfg(test)]
mod tests;
mod triangles;
mod view;

pub use art::{Avatar, LoadError, load_folder};
#[cfg(feature = "dev-tools")]
pub use demo::run_stage;
pub use pixmap::Pixmap;
pub use raster::{Picture, Rasteriser};
pub use view::{AvatarStage, Drive, Driver, Resting};
