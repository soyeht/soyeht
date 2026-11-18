//! The avatar of a workspace on the Windows and Linux app (ADR 0166, slice 5).
//!
//! The rules belong to the crate `soyeht-avatar` and the drawing to `soyeht-avatar-raster`; this
//! module is the window's part: `view`, the stage, a GPUI view that asks a `Driver` what the avatar
//! is doing, has the picture drawn on a background thread at the pace and the size the cost of the
//! last frames allows, and paints the finished picture.

#[cfg(feature = "dev-tools")]
mod demo;
mod view;

#[cfg(feature = "dev-tools")]
pub use demo::run_stage;
pub use soyeht_avatar_raster::{Avatar, LoadError, load_folder};
pub use view::{AvatarStage, Drive, Driver, Resting};
