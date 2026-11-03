//! A device that is not the computer's own: the phone (ADR 0013, 0132, 0134). It dials the engine's
//! WebSocket over the tailnet, proves itself with the key it made when it paired, and checks that the
//! engine holds the key its pairing link named. The bytes both sides sign are
//! `soyeht_protocol::device_auth`; what is here is the order and the transport.

mod credential;
mod failure;
mod link;
mod pairing;
mod websocket;

pub use credential::{Credential, Expected};
pub use failure::PairingFailure;
pub use link::{PairingLink, parse as parse_link};
pub use pairing::{Progress, pair};
pub(crate) use websocket::connect;

use base64::{Engine as _, engine::general_purpose::STANDARD};

/// Thirty-two bytes sent as standard base64, the way the wire carries a nonce or a challenge.
pub(crate) fn bytes32(text: &str) -> Option<[u8; 32]> {
    STANDARD.decode(text).ok()?.try_into().ok()
}
