#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! The image catalogue and the frame tooling of an avatar (ADR 0166, slice 9a).
//!
//! An avatar is built from one picture and 18 edits of it. The person chooses the image model that
//! makes the edits from a catalogue of providers, local and cloud; nothing here names a model.
//!
//! - `catalogue`: the descriptors (data) and the choice of one call: its address and model.
//! - `adapter`: how one edit reaches a provider (the OpenAI image-edit request, a ComfyUI server)
//!   and how the picture comes back. Requests are made by the bounded runner's `curl`
//!   (`soyeht_process::curl`), with the person's own key on standard input.
//! - `vault`: the person's own key for a provider that needs one, per address, on the computer
//!   that builds.
//! - `frames`: the frame spec, as data: the 18 frames, their prompts and how much may change.
//! - `check`: the deterministic check of an edited frame against its source.
//!
//! What an agent does from plain instructions (look at a frame, redo the ones that are wrong,
//! put the set together) is the `avatar-builder` skill's job, not this crate's.

mod adapter;
mod catalogue;
mod check;
mod error;
mod exchange;
mod frames;
#[cfg(feature = "test-support")]
pub mod testing;
mod vault;

pub use adapter::{Edit, edit};
pub use catalogue::{Adapter, Choice, Descriptor, find, host_of, registry};
pub use check::{Picture, Problem, Rect, Report, check};
pub use error::{Error, Result};
pub use frames::{Frame, Spec, spec};
pub use soyeht_platform::secret::Secret;
pub use vault::Vault;
