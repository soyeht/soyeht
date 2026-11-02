use soyeht_platform::log::Failure;
use std::fmt;

/// Why a device could not pair with, or trust, an engine. The text is for the person; the log gets
/// only [`Failure::failure_kind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingFailure {
    /// The text is not a pairing link of this product.
    InvalidLink,
    /// The Mac writes the three-field link of an engine before ADR 0132.
    OldLink,
    /// The name the person gave the device is empty or too long.
    InvalidName,
    /// The engine sent something that is not the pairing exchange.
    Unexpected,
    /// The engine did not prove it holds the key the link named.
    NotYourMac,
    /// The engine sent no proof: it predates ADR 0132.
    EngineTooOld,
    /// The person did not allow access, or the window closed.
    Refused(String),
    /// The saved access has no engine key (made before ADR 0132) or names another engine.
    PairAgain,
    /// The pairing window ran out.
    Expired,
}

impl fmt::Display for PairingFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLink => "This is not a valid Soyeht pairing link.",
            Self::OldLink | Self::EngineTooOld => {
                "Update Soyeht on the computer, then pair with its new link."
            }
            Self::InvalidName => "Give this device a name of up to 128 bytes.",
            Self::Unexpected => "The computer did not answer the way Soyeht does.",
            Self::NotYourMac => {
                "This is not your computer: the server at this address could not prove it holds \
                 the key this device trusts."
            }
            Self::Refused(reason) => return formatter.write_str(reason),
            Self::PairAgain => "This device cannot check your computer. Pair again.",
            Self::Expired => "The pairing link expired. Make a new one on the computer.",
        })
    }
}

impl std::error::Error for PairingFailure {}

impl Failure for PairingFailure {
    fn failure_kind(&self) -> String {
        match self {
            Self::InvalidLink => "invalid_pairing_link",
            Self::OldLink => "old_pairing_link",
            Self::InvalidName => "invalid_device_name",
            Self::Unexpected => "unexpected_pairing_message",
            Self::NotYourMac => "not_your_mac",
            Self::EngineTooOld => "engine_too_old",
            Self::Refused(_) => "pairing_refused",
            Self::PairAgain => "pair_again",
            Self::Expired => "pairing_expired",
        }
        .to_owned()
    }
}
