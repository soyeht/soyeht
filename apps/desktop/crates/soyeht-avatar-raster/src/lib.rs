//! The software rasteriser of the avatar (ADR 0166, slice 5): a package folder decoded into an
//! `Avatar`, and a `Rasteriser` that turns a frame of the runtime into BGRA pixels on one core.
//! It has no window and no GPUI: the desktop's stage (`soyeht-desktop`, module `avatar`) draws
//! with it on a background thread and paints what it makes.
//!
//! Every rule belongs to the crate `soyeht-avatar`: the package format and its bounds, the
//! continuous mouth, the acting layer, the pose, the eye frame and the order the parts are drawn
//! in, and how a part is laid over a picture (`composite`). This crate only draws them:
//!
//! - `art`: a package folder, validated and decoded into an `Avatar`;
//! - `raster`: a frame, composed at the size of the picture and read through the pose;
//! - `pace`: how often to draw and at what size, from what the last frames cost.
//!
//! The mouth is never a choice between pictures: the rasteriser draws the mesh at the geometry
//! the morph weights blend, so exactly one mouth is on screen in every frame.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod art;
mod pace;
mod pixmap;
mod raster;
mod resample;
#[cfg(test)]
mod tests;
mod triangles;

pub use art::{Avatar, LoadError, load_folder};
pub use pace::{NARROWEST, Pace, Pacer, WIDEST};
pub use pixmap::Pixmap;
pub use raster::{Picture, Rasteriser};
