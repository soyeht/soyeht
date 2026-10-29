use soyeht_platform::fs::PrivateDirError;
use std::{fmt, io};

/// Why a service could not be rendered, registered or asked about.
#[derive(Debug)]
pub enum Error {
    /// A value cannot be written into the system's file: it holds a control character, or a
    /// path that is not text.
    Unrepresentable(&'static str),
    /// A name or location is the legacy app's, or lacks "next" where it must carry it (ADR
    /// 0112). The text says which value and why.
    Legacy(String),
    /// The user's home folder, or the account, cannot be found.
    Environment(&'static str),
    /// A link stands where a folder or file the seam writes into must be.
    Unsafe(&'static str),
    /// A program the definition would start is not there; registering it would make the system
    /// restart a failing start for ever.
    ProgramMissing(String),
    /// The service is not registered, so it cannot be started.
    NotRegistered,
    /// The engine or the supervisor still answers on its endpoint, so its data is in use.
    StillRunning,
    /// The data directory is not one that may be deleted; the text says why.
    DataRefused(String),
    Io(io::Error),
    /// A command could not run or ran too long.
    Process(soyeht_process::Error),
    /// A command ran and said no. `detail` is what it printed on standard error.
    Failed {
        step: &'static str,
        detail: String,
    },
}

impl Error {
    /// What went wrong, in words for the person who ran the command.
    fn message(&self) -> String {
        match self {
            Self::Unrepresentable(what) => format!("{what} cannot be written into a service file"),
            Self::Legacy(reason) => format!("{reason}; the legacy app is never touched"),
            Self::Environment(what) => format!("{what} is unknown"),
            Self::Unsafe(what) => format!("{what} is a link; refusing to write through it"),
            Self::ProgramMissing(program) => {
                format!("the program {program} does not exist; check --programs")
            }
            Self::NotRegistered => "the service is not registered".to_owned(),
            Self::StillRunning => "the engine or the supervisor still answers; quit the app or \
                 program that runs it, then run the command again (the services are unregistered \
                 and the data is untouched)"
                .to_owned(),
            Self::DataRefused(reason) => {
                format!("refusing to delete the data: {reason}; nothing was deleted")
            }
            Self::Io(error) => format!("file error: {error}"),
            Self::Process(error) => format!("command error: {error}"),
            Self::Failed { step, detail } if detail.is_empty() => format!("{step} failed"),
            Self::Failed { step, detail } => format!("{step} failed: {detail}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<PrivateDirError> for Error {
    fn from(error: PrivateDirError) -> Self {
        match error {
            PrivateDirError::Unsafe => Self::Unsafe("a folder"),
            other @ (PrivateDirError::NotPrivate | PrivateDirError::Io(_)) => {
                Self::Io(other.into())
            }
        }
    }
}

impl From<soyeht_process::Error> for Error {
    fn from(error: soyeht_process::Error) -> Self {
        Self::Process(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
