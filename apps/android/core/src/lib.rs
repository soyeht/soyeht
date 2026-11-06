//! The Android app's core (ADR 0134): the connection, the pairing and the replica state of a phone
//! that is a paired device of the person's computer. The views are Kotlin and call what this crate
//! exports through UniFFI; every rule is `soyeht-client`'s, which the desktop client uses too.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod composer;
mod conversation;
mod core;
mod error;
mod link;
mod markdown;
mod overview;
mod terminal;
mod usage;

pub use core::{Core, Listener, device_name};
pub use error::CoreError;
pub use link::{PairingOffer, pairing_offer};
pub use markdown::{CodeCopy, MarkdownBlock, code_copy, markdown_blocks};
pub use usage::{GaugeBand, UsageGauge};

uniffi::setup_scaffolding!();
