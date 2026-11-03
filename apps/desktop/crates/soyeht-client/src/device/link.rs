//! The pairing link: its grammar and the addresses it may name are `soyeht_protocol::pairing_link`'s,
//! which the engine writes it with. This only says a failure in the person's words.

use super::PairingFailure;
use soyeht_protocol::pairing_link::LinkError;
pub use soyeht_protocol::pairing_link::PairingLink;

pub fn parse(text: &str) -> Result<PairingLink, PairingFailure> {
    PairingLink::parse(text).map_err(|error| match error {
        LinkError::Old => PairingFailure::OldLink,
        LinkError::Invalid => PairingFailure::InvalidLink,
    })
}
