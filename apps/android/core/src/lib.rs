//! The Android app's core (ADR 0134): the connection, the pairing and the replica state of a phone
//! that is a paired device of the person's computer. The views are Kotlin and call what this crate
//! exports through UniFFI; every rule is `soyeht-client`'s, which the desktop client uses too.

mod conversation;
mod core;
mod error;
mod overview;
mod terminal;

pub use core::{Core, Listener, device_name};
pub use error::CoreError;

uniffi::setup_scaffolding!();
