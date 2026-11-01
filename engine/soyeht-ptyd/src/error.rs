use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Protocol(soyeht_protocol::Error),
    Json(serde_json::Error),
    Base64(base64::DecodeError),
    Remote {
        kind: String,
        message: String,
    },
    InvalidWelcome,
    InvalidResponse,
    ZeroRetentionLimit,
    OutputOffsetExhausted,
    AckAhead,
    CursorAhead,
    PendingRequestNotFound,
    InvalidPendingOffset,
    PoisonedLock(&'static str),
    AlreadyRunning,
    SocketPathNotSocket,
    ConnectionSuperseded,
    RegistryInstanceMissing,
    InvalidBase64Input,
    InvalidSpawnParameters,
    EnvironmentVariableNotAllowed,
    InvalidTerminalSize,
    MissingGuardianResponse,
    GuardianFailed(String),
    /// The pipe guardian could not start the program because the system refused it.
    GuardianRefused(String),
    InvalidGuardianResponse,
    GuardianStdinMissing,
    GuardianStdoutMissing,
    InvalidGuardianDescriptor,
    GuardianNotSessionLeader,
    EmptyCommand,
    IncompatibleRegistryVersion,
    SupervisorGenerationExhausted,
    InvalidGuardianFd,
    InvalidArguments,
}

/// Whether the system refused to start a program for a reason that does not pass: the folder
/// or the program is not there, or is not allowed. Running out of a resource (processes,
/// descriptors, disk) or a timeout is not one, and may pass.
pub fn refuses_to_start(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory | io::ErrorKind::PermissionDenied
    )
}

/// An `io::Error` from the setup around a spawn (the diagnostic log, the job, the console),
/// which says nothing about the folder or the program: its kind is made one that does not
/// count as a refusal, so a missing log folder is not taken for a missing working folder.
pub(crate) fn setup_failed(error: io::Error) -> io::Error {
    if refuses_to_start(error.kind()) {
        io::Error::other(error)
    } else {
        error
    }
}

impl Error {
    /// A failure of the setup around starting a pipe process or its guardian (the
    /// diagnostic log, the supervisor's own executable, the guardian's start). It is never a
    /// refusal, whatever its kind.
    pub(crate) fn guardian_setup(error: io::Error) -> Self {
        Self::GuardianFailed(error.to_string())
    }

    /// A start failure of the guardian or of a pipe process, kept apart by whether it can pass.
    pub fn guardian(error: &io::Error) -> Self {
        let message = error.to_string();
        if refuses_to_start(error.kind()) {
            Self::GuardianRefused(message)
        } else {
            Self::GuardianFailed(message)
        }
    }

    /// Whether this failure to spawn is a refusal that repeating cannot fix.
    pub fn spawn_refused(&self) -> bool {
        match self {
            Self::Io(error) => refuses_to_start(error.kind()),
            Self::GuardianRefused(_) => true,
            _ => false,
        }
    }

    pub fn wire_kind(&self) -> &'static str {
        match self {
            Self::AckAhead => crate::wire_kind::ACK_AHEAD,
            Self::CursorAhead => crate::wire_kind::CURSOR_AHEAD,
            Self::PendingRequestNotFound => crate::wire_kind::NOT_FOUND,
            Self::InvalidPendingOffset => crate::wire_kind::INVALID_OFFSET,
            Self::OutputOffsetExhausted => crate::wire_kind::OUTPUT_OFFSET_OVERFLOW,
            _ => crate::wire_kind::OPERATION_FAILED,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Protocol(error) => write!(formatter, "protocol: {error}"),
            Self::Json(error) => write!(formatter, "JSON: {error}"),
            Self::Base64(error) => write!(formatter, "base64: {error}"),
            Self::Remote { kind, message } => write!(formatter, "{kind}: {message}"),
            Self::InvalidWelcome => formatter.write_str("invalid ptyd welcome"),
            Self::InvalidResponse => formatter.write_str("invalid ptyd response"),
            Self::ZeroRetentionLimit => formatter.write_str("retention limit is zero"),
            Self::OutputOffsetExhausted => formatter.write_str("output offset exhausted"),
            Self::AckAhead => formatter.write_str("acknowledgement is ahead of output"),
            Self::CursorAhead => formatter.write_str("cursor is ahead of output"),
            Self::PendingRequestNotFound => formatter.write_str("pending request not found"),
            Self::InvalidPendingOffset => formatter.write_str("invalid pending request offset"),
            Self::PoisonedLock(resource) => write!(formatter, "{resource} lock is poisoned"),
            Self::AlreadyRunning => formatter.write_str("ptyd is already running"),
            Self::SocketPathNotSocket => formatter.write_str("socket path is not a socket"),
            Self::ConnectionSuperseded => formatter.write_str("connection superseded"),
            Self::RegistryInstanceMissing => formatter.write_str("registry instance disappeared"),
            Self::InvalidBase64Input => formatter.write_str("invalid base64 input"),
            Self::InvalidSpawnParameters => formatter.write_str("invalid spawn parameters"),
            Self::EnvironmentVariableNotAllowed => {
                formatter.write_str("environment variable is not allowed")
            }
            Self::InvalidTerminalSize => formatter.write_str("invalid terminal size"),
            Self::MissingGuardianResponse => formatter.write_str("missing pipe guardian response"),
            Self::GuardianFailed(message) | Self::GuardianRefused(message) => {
                write!(formatter, "pipe guardian failed: {message}")
            }
            Self::InvalidGuardianResponse => formatter.write_str("invalid pipe guardian response"),
            Self::GuardianStdinMissing => {
                formatter.write_str("pipe guardian stdin was not created")
            }
            Self::GuardianStdoutMissing => {
                formatter.write_str("pipe guardian stdout was not created")
            }
            Self::InvalidGuardianDescriptor => {
                formatter.write_str("invalid pipe guardian descriptor")
            }
            Self::GuardianNotSessionLeader => {
                formatter.write_str("pipe guardian must own its session")
            }
            Self::EmptyCommand => formatter.write_str("empty command"),
            Self::IncompatibleRegistryVersion => {
                formatter.write_str("incompatible registry version")
            }
            Self::SupervisorGenerationExhausted => {
                formatter.write_str("supervisor generation exhausted")
            }
            Self::InvalidGuardianFd => formatter.write_str("invalid pipe guardian descriptor"),
            Self::InvalidArguments => {
                formatter.write_str("usage: soyeht-ptyd [--data-dir <isolated-directory>]")
            }
        }
    }
}

/// A failure of the supervisor logs as a bounded label, never as message text: the name of the
/// variant (the payload of a few holds a path or a system message), and the `io::ErrorKind` of an
/// I/O error.
impl soyeht_platform::log::Failure for Error {
    fn failure_kind(&self) -> String {
        match self {
            Self::Io(error) => format!("io:{:?}", error.kind()),
            other => format!("{other:?}")
                .chars()
                .take_while(|character| character.is_alphanumeric() || *character == '_')
                .collect(),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Protocol(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Base64(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<soyeht_protocol::Error> for Error {
    fn from(error: soyeht_protocol::Error) -> Self {
        Self::Protocol(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<base64::DecodeError> for Error {
    fn from(error: base64::DecodeError) -> Self {
        Self::Base64(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_missing_or_forbidden_folder_or_program_is_a_refusal() {
        for kind in [
            io::ErrorKind::NotFound,
            io::ErrorKind::NotADirectory,
            io::ErrorKind::PermissionDenied,
        ] {
            assert!(Error::Io(kind.into()).spawn_refused(), "{kind:?}");
            assert!(matches!(
                Error::guardian(&kind.into()),
                Error::GuardianRefused(_)
            ));
        }
        // Resource and timing failures may pass.
        for kind in [
            io::ErrorKind::WouldBlock,
            io::ErrorKind::OutOfMemory,
            io::ErrorKind::StorageFull,
            io::ErrorKind::TimedOut,
            io::ErrorKind::Other,
        ] {
            assert!(!Error::Io(kind.into()).spawn_refused(), "{kind:?}");
            assert!(matches!(
                Error::guardian(&kind.into()),
                Error::GuardianFailed(_)
            ));
        }
        assert!(!Error::Io(setup_failed(io::ErrorKind::NotFound.into())).spawn_refused());
        assert!(!Error::guardian_setup(io::ErrorKind::PermissionDenied.into()).spawn_refused());
        assert!(!Error::MissingGuardianResponse.spawn_refused());
        assert!(!Error::GuardianFailed("x".into()).spawn_refused());
    }

    #[test]
    fn a_failure_logs_its_kind_and_never_the_text_of_a_path() {
        use soyeht_platform::log::Failure as _;
        let refused = Error::GuardianRefused("/Users/dana/Projects/secret: not allowed".into());
        assert_eq!(refused.failure_kind(), "GuardianRefused");
        assert_eq!(Error::AlreadyRunning.failure_kind(), "AlreadyRunning");
        assert_eq!(
            Error::Io(io::ErrorKind::PermissionDenied.into()).failure_kind(),
            "io:PermissionDenied"
        );
        assert_eq!(Error::PoisonedLock("state").failure_kind(), "PoisonedLock");
        let remote = Error::Remote {
            kind: "kind".into(),
            message: "secret detail".into(),
        };
        assert_eq!(remote.failure_kind(), "Remote");
        assert_eq!(
            Error::ConnectionSuperseded.failure_kind(),
            "ConnectionSuperseded"
        );
        let broken = Error::Io(io::Error::new(io::ErrorKind::BrokenPipe, "/private/path"));
        assert_eq!(broken.failure_kind(), "io:BrokenPipe");
    }
}
