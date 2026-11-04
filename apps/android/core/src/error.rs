use soyeht_client::{Error, device::PairingFailure};
use soyeht_platform::log::Failure;

/// What a call of the core can fail with: the text is for the person, and the kind is what the
/// views branch on.
#[derive(Debug, uniffi::Error)]
pub enum CoreError {
    /// The pairing link or the saved access is not usable; the person has to pair again.
    PairAgain { reason: String },
    /// The pairing was refused, expired, or could not reach the computer.
    Failed { reason: String },
    /// The person stopped the pairing.
    Cancelled,
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PairAgain { reason } | Self::Failed { reason } => formatter.write_str(reason),
            Self::Cancelled => formatter.write_str(&PairingFailure::Cancelled.to_string()),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<Error> for CoreError {
    fn from(error: Error) -> Self {
        soyeht_platform::log::failure("android.core_call_failed", &error);
        let message = error.to_string();
        if matches!(error, Error::Pairing(PairingFailure::Cancelled)) {
            Self::Cancelled
        } else if soyeht_client::device::Credential::refused_by(&error) {
            Self::PairAgain { reason: message }
        } else {
            Self::Failed { reason: message }
        }
    }
}

impl Failure for CoreError {
    fn failure_kind(&self) -> String {
        match self {
            Self::PairAgain { .. } => "pair_again",
            Self::Failed { .. } => "failed",
            Self::Cancelled => "cancelled",
        }
        .to_owned()
    }
}
