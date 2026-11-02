use soyeht_client::Error;
use soyeht_platform::log::Failure;

/// What a call of the core can fail with: the text is for the person, and the kind is what the
/// views branch on.
#[derive(Debug, uniffi::Error)]
pub enum CoreError {
    /// The pairing link or the saved access is not usable; the person has to pair again.
    PairAgain { message: String },
    /// The pairing was refused, expired, or could not reach the computer.
    Failed { message: String },
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PairAgain { message } | Self::Failed { message } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<Error> for CoreError {
    fn from(error: Error) -> Self {
        soyeht_platform::log::failure("android.core_call_failed", &error);
        let message = error.to_string();
        if soyeht_client::device::Credential::refused_by(&error) {
            Self::PairAgain { message }
        } else {
            Self::Failed { message }
        }
    }
}

impl Failure for CoreError {
    fn failure_kind(&self) -> String {
        match self {
            Self::PairAgain { .. } => "pair_again",
            Self::Failed { .. } => "failed",
        }
        .to_owned()
    }
}
