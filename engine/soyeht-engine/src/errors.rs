use serde::Serialize;
use serde_json::Value;
use soyeht_protocol::{
    ErrorData, ErrorKind, RpcError, data_directory::RESTART_SUPERVISOR_FLAG, error_code,
};
use soyeht_ptyd::wire_kind;
use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    NotFound(String),
    InvalidParams(String),
    Unauthorized(String),
    ForbiddenRole(String),
    PaneBusy(String),
    TaskHasOwner(String),
    TransitionRefused(String),
    StaleAttempt(String),
    StaleVersion(String),
    DependencyCycle(String),
    DependencyOpen(String),
    RateLimited {
        message: String,
        retry_after_sec: u32,
    },
    IdempotencyConflict(String),
    HarnessUnavailable(String),
    HarnessSwitchFailed(String),
    NeedsLogin(String),
    CursorTooOld(String),
    ResyncRequired(String),
    SlowConsumer(String),
    ProtocolVersionUnsupported(String),
    DiskFull(String),
    PtydUnavailable(String),
    StaleEngineGeneration(String),
    WorktreeUnavailable(String),
    GitUnavailable(String),
    WebHostUnavailable(String),
    WritingKindRefused(String),
    AttachmentLimitExceeded(String),
    PlatformUnsupported(String),
    FileNotText(String),
    FileTooLarge(String),
    Internal(String),
    SnapshotTooLarge(String),
    Storage(soyeht_store::Error),
    Io(io::Error),
    Acp(soyeht_acp::Error),
    Editor(soyeht_editor::Error),
    Lsp(soyeht_lsp::Error),
    PaneState(soyeht_pane_state::Error),
    Vcs(soyeht_vcs::Error),
    Web(soyeht_web::Error),
    PortScan(crate::ports::PortScanError),
    Writing(soyeht_writing::Error),
    Supervisor(soyeht_ptyd::Error),
    Protocol(soyeht_protocol::Error),
    WebSocket(tokio_tungstenite::tungstenite::Error),
    Utf8(std::string::FromUtf8Error),
    Address(std::net::AddrParseError),
    Join(tokio::task::JoinError),
    EngineStatePoisoned,
    AlreadyRunning,
    ListenerAddressNotPermitted,
    SocketPathNotSocket,
    MissingDataDirectoryArgument,
    MissingListenAddressArgument,
    MissingSupervisorProgramArgument,
    InvalidArguments,
    InvalidBinaryWebSocketFrame,
    RandomUnavailable,
    InvalidPublicKey,
    InvalidPublicKeyLength,
    WeakPublicKey,
    ConnectionClosed,
    RecordedRpc(RpcError),
}

pub type Result<T> = std::result::Result<T, Error>;

macro_rules! constructor {
    ($name:ident, $variant:ident) => {
        pub fn $name(message: impl Into<String>) -> Error {
            Error::$variant(message.into())
        }
    };
}

constructor!(not_found, NotFound);
constructor!(invalid, InvalidParams);
constructor!(unauthorized, Unauthorized);
constructor!(forbidden_role, ForbiddenRole);
constructor!(pane_busy, PaneBusy);
constructor!(task_has_owner, TaskHasOwner);
constructor!(transition_refused, TransitionRefused);
constructor!(stale_attempt, StaleAttempt);
constructor!(stale_version, StaleVersion);
constructor!(dependency_cycle, DependencyCycle);
constructor!(dependency_open, DependencyOpen);
constructor!(idempotency_conflict, IdempotencyConflict);
constructor!(harness_unavailable, HarnessUnavailable);
constructor!(harness_switch_failed, HarnessSwitchFailed);
constructor!(needs_login, NeedsLogin);
constructor!(cursor_too_old, CursorTooOld);
constructor!(resync_required, ResyncRequired);
constructor!(slow_consumer, SlowConsumer);
constructor!(protocol_version_unsupported, ProtocolVersionUnsupported);
constructor!(disk_full, DiskFull);
constructor!(ptyd_unavailable, PtydUnavailable);
constructor!(stale_engine_generation, StaleEngineGeneration);
constructor!(worktree_unavailable, WorktreeUnavailable);
constructor!(git_unavailable, GitUnavailable);

/// The error of a `git` or `wt` command that did not run: `git_unavailable` when the machine
/// has no usable `git`, `worktree_unavailable` for any other reason.
pub fn worktree_process(error: soyeht_process::Error) -> Error {
    match error {
        soyeht_process::Error::GitUnavailable => git_unavailable(error.to_string()),
        other => worktree_unavailable(other.to_string()),
    }
}
constructor!(web_host_unavailable, WebHostUnavailable);
constructor!(writing_kind_refused, WritingKindRefused);
constructor!(attachment_limit_exceeded, AttachmentLimitExceeded);
constructor!(platform_unsupported, PlatformUnsupported);
constructor!(file_not_text, FileNotText);
constructor!(file_too_large, FileTooLarge);
constructor!(internal, Internal);
constructor!(snapshot_too_large, SnapshotTooLarge);

pub fn rate_limited(message: impl Into<String>, retry_after_sec: u32) -> Error {
    Error::RateLimited {
        message: message.into(),
        retry_after_sec,
    }
}

pub fn required<T>(value: Option<T>, message: &'static str) -> Result<T> {
    value.ok_or_else(|| invalid(message))
}

pub fn param_str<'a>(params: &'a Value, name: &str) -> Result<&'a str> {
    params[name]
        .as_str()
        .ok_or_else(|| invalid(format!("{name} must be a string")))
}

pub fn param_bool(params: &Value, name: &str) -> Result<bool> {
    params[name]
        .as_bool()
        .ok_or_else(|| invalid(format!("{name} must be a boolean")))
}

pub fn param_u64(params: &Value, name: &str) -> Result<u64> {
    params[name]
        .as_u64()
        .ok_or_else(|| invalid(format!("{name} must be an unsigned integer")))
}

pub fn invariant<T>(value: Option<T>, message: &'static str) -> Result<T> {
    value.ok_or_else(|| internal(message))
}

pub fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| internal("JSON serialization failed"))
}

pub fn storage(error: soyeht_store::Error) -> Error {
    Error::Storage(error)
}

pub fn io(error: io::Error) -> Error {
    Error::Io(error)
}

pub fn lsp(error: soyeht_lsp::Error) -> Error {
    Error::Lsp(error)
}

fn store_kind(error: &soyeht_store::Error) -> ErrorKind {
    use soyeht_store::Error;
    match error {
        Error::IdempotencyConflict => ErrorKind::IdempotencyConflict,
        Error::CursorTooOld => ErrorKind::CursorTooOld,
        Error::DiskFull => ErrorKind::DiskFull,
        Error::TaskVersionConflict => ErrorKind::StaleVersion,
        Error::TaskDependencyCycle => ErrorKind::DependencyCycle,
        Error::TaskReservationConflict => ErrorKind::TaskHasOwner,
        Error::SubitemNotFound => ErrorKind::NotFound,
        Error::FileVersionNotFound => ErrorKind::NotFound,
        Error::FileReplacementBusy => ErrorKind::StaleVersion,
        Error::InvalidEngineGeneration
        | Error::InvalidIdempotencyKey
        | Error::EventParametersNotObject
        | Error::CursorAhead
        | Error::NegativeEventId
        | Error::NegativeEventCursor
        | Error::PairingRequestLimit
        | Error::InvalidQueuedPrompt
        | Error::PromptQueueFull
        | Error::AttentionItemTooLarge
        | Error::AttentionPaneMismatch
        | Error::AgentRuntimeTooLarge
        | Error::StaleAgentGeneration
        | Error::NewOutboxHasProgress
        | Error::IncompleteOutboxRecord
        | Error::InvalidOutboxEnvelope
        | Error::AgentOutboxFull
        | Error::InvalidPartialWriteReceipt
        | Error::NativeWriteAttemptsExhausted
        | Error::NativeMessageIncomplete
        | Error::InvalidNativeRequestIdentity
        | Error::PublicRequestTooLarge
        | Error::PendingRequestsTooLarge
        | Error::NativeRequestNotFound
        | Error::NativeEffectNotFound
        | Error::NativeEffectNotPending
        | Error::NativeEffectAlreadyStarted
        | Error::InvalidTranscriptSequence
        | Error::TranscriptMessageTooLarge
        | Error::TranscriptMutation
        | Error::StaleTranscriptReplay
        | Error::InvalidTranscriptPageLimit
        | Error::TranscriptPageTooLarge
        | Error::ApprovedPairTokenMissing => ErrorKind::InvalidParams,
        Error::InvalidEditorFile
        | Error::InvalidFileHistoryLimit
        | Error::InvalidFileReplacement => ErrorKind::InvalidParams,
        _ => ErrorKind::InternalError,
    }
}

fn supervisor_kind(error: &soyeht_ptyd::Error) -> ErrorKind {
    match error {
        soyeht_ptyd::Error::Remote { kind, .. } if kind == wire_kind::STALE_ENGINE_GENERATION => {
            ErrorKind::StaleEngineGeneration
        }
        soyeht_ptyd::Error::Remote { kind, .. }
            if kind == wire_kind::SPAWN_FAILED || kind == wire_kind::SPAWN_REFUSED =>
        {
            ErrorKind::HarnessUnavailable
        }
        _ => ErrorKind::PtydUnavailable,
    }
}

fn editor_kind(error: &soyeht_editor::Error) -> ErrorKind {
    match error {
        soyeht_editor::Error::ParserLanguage | soyeht_editor::Error::ParserStopped => {
            ErrorKind::InternalError
        }
        _ => ErrorKind::InvalidParams,
    }
}

fn vcs_kind(error: &soyeht_vcs::Error) -> ErrorKind {
    match error {
        soyeht_vcs::Error::InvalidRepository | soyeht_vcs::Error::InvalidPath => {
            ErrorKind::InvalidParams
        }
        soyeht_vcs::Error::GitUnavailable => ErrorKind::GitUnavailable,
        _ => ErrorKind::InternalError,
    }
}

fn web_kind(error: &soyeht_web::Error) -> ErrorKind {
    match error {
        soyeht_web::Error::EmptyUrl
        | soyeht_web::Error::InvalidUrl
        | soyeht_web::Error::UnsupportedScheme
        | soyeht_web::Error::MissingHost
        | soyeht_web::Error::InvalidViewport
        | soyeht_web::Error::InvalidWait
        | soyeht_web::Error::InvalidCommand => ErrorKind::NavigationRefused,
        soyeht_web::Error::StaleReference => ErrorKind::StaleReference,
        soyeht_web::Error::HumanModeUnavailable => ErrorKind::WebHostUnavailable,
        _ => ErrorKind::InternalError,
    }
}

fn writing_kind(error: &soyeht_writing::Error) -> ErrorKind {
    use soyeht_writing::Error;
    match error {
        Error::InvalidSectionPath => ErrorKind::WritingSectionRefused,
        Error::VariantNotFound | Error::CannotRemoveOnlyVariant => ErrorKind::WritingLineNotFound,
        Error::BeatNotFound => ErrorKind::NotFound,
        Error::SceneNotFound => ErrorKind::WritingSceneNotFound,
        Error::DocumentTooLarge
        | Error::SlideLimit
        | Error::SceneLimit
        | Error::ElementLimit
        | Error::VariantLimit
        | Error::VariantTooLarge
        | Error::BeatLimit
        | Error::LinkLimit
        | Error::SectionLimit => ErrorKind::WritingLimitExceeded,
        Error::InvalidPosition
        | Error::InvalidRange
        | Error::EmptySelection
        | Error::MissingFormatValue
        | Error::InvalidVariantSelection
        | Error::InvalidBeat => ErrorKind::InvalidParams,
    }
}

/// An engine failure logs as its bounded protocol kind, never as message text. An I/O error
/// logs its `io::ErrorKind` too: the protocol kind of every one is `internal_error`, which
/// would hide whether a file was missing or refused.
impl soyeht_platform::log::Failure for Error {
    fn failure_kind(&self) -> String {
        match self {
            Self::Io(error) => format!("io:{:?}", error.kind()),
            _ => wire_name(&kind(self)),
        }
    }
}

/// The name a wire value has in its JSON form, such as an error kind or a reason.
pub fn wire_name(value: &(impl Serialize + ?Sized)) -> String {
    serde_json::to_value(value).map_or_else(
        |error| error.to_string(),
        |value| {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        },
    )
}

/// Logs the failure of a result the caller cannot return, as one structured line.
pub fn log_if_failed(event: &str, result: Result<()>) {
    if let Err(error) = result {
        soyeht_platform::log::failure(event, &error);
    }
}

/// The single translation from engine failures to the public protocol.
pub fn kind(error: &Error) -> ErrorKind {
    match error {
        Error::RecordedRpc(error) => error.data.kind.clone(),
        Error::NotFound(_) => ErrorKind::NotFound,
        Error::InvalidParams(_) => ErrorKind::InvalidParams,
        Error::Unauthorized(_) => ErrorKind::Unauthorized,
        Error::ForbiddenRole(_) => ErrorKind::ForbiddenRole,
        Error::PaneBusy(_) => ErrorKind::PaneBusy,
        Error::TaskHasOwner(_) => ErrorKind::TaskHasOwner,
        Error::TransitionRefused(_) => ErrorKind::TransitionRefused,
        Error::StaleAttempt(_) => ErrorKind::StaleAttempt,
        Error::StaleVersion(_) => ErrorKind::StaleVersion,
        Error::DependencyCycle(_) => ErrorKind::DependencyCycle,
        Error::DependencyOpen(_) => ErrorKind::DependencyOpen,
        Error::RateLimited { .. } => ErrorKind::RateLimited,
        Error::IdempotencyConflict(_) => ErrorKind::IdempotencyConflict,
        Error::HarnessUnavailable(_) | Error::Acp(_) | Error::Lsp(_) => {
            ErrorKind::HarnessUnavailable
        }
        Error::HarnessSwitchFailed(_) => ErrorKind::HarnessSwitchFailed,
        Error::Editor(error) => editor_kind(error),
        Error::Vcs(error) => vcs_kind(error),
        Error::Web(error) => web_kind(error),
        Error::PortScan(crate::ports::PortScanError::Unsupported(_)) => {
            ErrorKind::PlatformUnsupported
        }
        Error::PortScan(_) => ErrorKind::InternalError,
        Error::Writing(error) => writing_kind(error),
        Error::NeedsLogin(_) => ErrorKind::NeedsLogin,
        Error::CursorTooOld(_) => ErrorKind::CursorTooOld,
        Error::ResyncRequired(_) => ErrorKind::ResyncRequired,
        Error::SlowConsumer(_) => ErrorKind::SlowConsumer,
        Error::ProtocolVersionUnsupported(_) => ErrorKind::ProtocolVersionUnsupported,
        Error::DiskFull(_) => ErrorKind::DiskFull,
        Error::PtydUnavailable(_) => ErrorKind::PtydUnavailable,
        Error::StaleEngineGeneration(_) => ErrorKind::StaleEngineGeneration,
        Error::WorktreeUnavailable(_) => ErrorKind::InternalError,
        Error::GitUnavailable(_) => ErrorKind::GitUnavailable,
        Error::WebHostUnavailable(_) => ErrorKind::WebHostUnavailable,
        Error::WritingKindRefused(_) => ErrorKind::WritingKindRefused,
        Error::AttachmentLimitExceeded(_) => ErrorKind::AttachmentLimitExceeded,
        Error::PlatformUnsupported(_) => ErrorKind::PlatformUnsupported,
        Error::FileNotText(_) => ErrorKind::FileNotText,
        Error::FileTooLarge(_) => ErrorKind::FileTooLarge,
        Error::Internal(_)
        | Error::Protocol(_)
        | Error::WebSocket(_)
        | Error::Utf8(_)
        | Error::Address(_)
        | Error::Join(_)
        | Error::PaneState(_)
        | Error::EngineStatePoisoned
        | Error::AlreadyRunning
        | Error::SocketPathNotSocket
        | Error::RandomUnavailable
        | Error::ConnectionClosed => ErrorKind::InternalError,
        Error::ListenerAddressNotPermitted
        | Error::MissingDataDirectoryArgument
        | Error::MissingListenAddressArgument
        | Error::MissingSupervisorProgramArgument
        | Error::InvalidArguments
        | Error::InvalidBinaryWebSocketFrame
        | Error::InvalidPublicKey
        | Error::InvalidPublicKeyLength
        | Error::WeakPublicKey => ErrorKind::InvalidParams,
        Error::SnapshotTooLarge(_) => ErrorKind::SnapshotTooLarge,
        Error::Storage(error) => store_kind(error),
        Error::Io(error) if error.kind() == io::ErrorKind::StorageFull => ErrorKind::DiskFull,
        Error::Io(_) => ErrorKind::InternalError,
        Error::Supervisor(error) => supervisor_kind(error),
    }
}

/// Whether `error` says the link to the supervisor is gone (a transport or protocol failure),
/// as opposed to the supervisor answering with a refusal or a failure on a link that still
/// works. A storage error or a failure of the engine's own is not the link's either. This is
/// the one rule for dropping the link.
pub fn supervisor_link_lost(error: &Error) -> bool {
    matches!(
        error,
        Error::Supervisor(error) if !matches!(error, soyeht_ptyd::Error::Remote { .. })
    )
}

pub fn missing_supervisor_instance(error: &Error) -> bool {
    matches!(
        error,
        Error::Supervisor(soyeht_ptyd::Error::Remote { kind, .. }) if kind == wire_kind::NOT_FOUND
    )
}

/// Only boundaries shared by every pane may stop one polling cycle.
///
/// Native/protocol/state failures belong to the pane that produced them. The
/// caller records those through `fail_agent` and keeps polling independent
/// panes. Storage durability, supervisor transport/generation and exhausted
/// storage are global because continuing could publish state that the engine
/// cannot durably commit or reconcile.
pub fn agent_poll_failure_is_global(error: &Error) -> bool {
    match error {
        Error::Storage(_)
        | Error::DiskFull(_)
        | Error::PtydUnavailable(_)
        | Error::StaleEngineGeneration(_)
        | Error::Supervisor(_)
        | Error::EngineStatePoisoned => true,
        Error::Io(error) if error.kind() == io::ErrorKind::StorageFull => true,
        _ => false,
    }
}

impl From<soyeht_editor::Error> for Error {
    fn from(error: soyeht_editor::Error) -> Self {
        Self::Editor(error)
    }
}

impl From<soyeht_writing::Error> for Error {
    fn from(error: soyeht_writing::Error) -> Self {
        Self::Writing(error)
    }
}

fn message(error: &Error) -> String {
    match error {
        Error::NotFound(message)
        | Error::InvalidParams(message)
        | Error::Unauthorized(message)
        | Error::ForbiddenRole(message)
        | Error::PaneBusy(message)
        | Error::TaskHasOwner(message)
        | Error::TransitionRefused(message)
        | Error::StaleAttempt(message)
        | Error::StaleVersion(message)
        | Error::DependencyCycle(message)
        | Error::DependencyOpen(message)
        | Error::IdempotencyConflict(message)
        | Error::HarnessUnavailable(message)
        | Error::HarnessSwitchFailed(message)
        | Error::NeedsLogin(message)
        | Error::CursorTooOld(message)
        | Error::ResyncRequired(message)
        | Error::SlowConsumer(message)
        | Error::ProtocolVersionUnsupported(message)
        | Error::DiskFull(message)
        | Error::PtydUnavailable(message)
        | Error::StaleEngineGeneration(message)
        | Error::WorktreeUnavailable(message)
        | Error::GitUnavailable(message)
        | Error::WebHostUnavailable(message)
        | Error::WritingKindRefused(message)
        | Error::AttachmentLimitExceeded(message)
        | Error::PlatformUnsupported(message)
        | Error::FileNotText(message)
        | Error::FileTooLarge(message)
        | Error::Internal(message)
        | Error::SnapshotTooLarge(message) => message.clone(),
        Error::RateLimited { message, .. } => message.clone(),
        Error::Storage(soyeht_store::Error::IdempotencyConflict) => {
            "The idempotency key has different parameters".into()
        }
        Error::Storage(soyeht_store::Error::CursorTooOld) => "Request a new snapshot".into(),
        Error::Storage(soyeht_store::Error::DiskFull) => "Storage is full".into(),
        Error::Storage(error) if store_kind(error) == ErrorKind::InvalidParams => error.to_string(),
        Error::Storage(_) => "Storage operation failed".into(),
        Error::Io(error) if error.kind() == io::ErrorKind::StorageFull => "Storage is full".into(),
        Error::Io(_) => "I/O operation failed".into(),
        Error::Acp(error) => error.to_string(),
        Error::Editor(error) => error.to_string(),
        Error::Lsp(error) => error.to_string(),
        Error::PaneState(error) => error.to_string(),
        Error::Vcs(error) => error.to_string(),
        Error::Web(error) => error.to_string(),
        Error::PortScan(error) => error.to_string(),
        Error::Writing(error) => error.to_string(),
        Error::Supervisor(soyeht_ptyd::Error::Remote { message, .. }) => message.clone(),
        Error::Supervisor(_) => "Supervisor unavailable".into(),
        Error::Protocol(_) => "Protocol operation failed".into(),
        Error::WebSocket(_) => "WebSocket operation failed".into(),
        Error::Utf8(_) => "Invalid UTF-8".into(),
        Error::Address(_) => "Invalid listener address".into(),
        Error::Join(_) => "Engine worker failed".into(),
        Error::EngineStatePoisoned => "Engine state lock poisoned".into(),
        Error::AlreadyRunning => "Engine already running".into(),
        Error::ListenerAddressNotPermitted => {
            "Engine accepts only loopback or tailnet listener addresses".into()
        }
        Error::SocketPathNotSocket => "Engine socket path is not a socket".into(),
        Error::MissingDataDirectoryArgument => "--data-dir requires a path".into(),
        Error::MissingListenAddressArgument => "--listen requires an address".into(),
        Error::MissingSupervisorProgramArgument => {
            format!("{RESTART_SUPERVISOR_FLAG} requires the supervisor's program")
        }
        Error::InvalidArguments => "Expected --data-dir or --listen".into(),
        Error::InvalidBinaryWebSocketFrame => "Invalid binary WebSocket frame".into(),
        Error::RandomUnavailable => "Random source unavailable".into(),
        Error::InvalidPublicKey => "Invalid public key".into(),
        Error::InvalidPublicKeyLength => "Invalid public key length".into(),
        Error::WeakPublicKey => "Weak public key".into(),
        Error::ConnectionClosed => "Connection closed".into(),
        Error::RecordedRpc(error) => error.message.clone(),
    }
}

pub fn wire(error: Error) -> RpcError {
    if let Error::RecordedRpc(error) = error {
        return error;
    }
    let kind = kind(&error);
    let retry_after_sec = match &error {
        Error::RateLimited {
            retry_after_sec, ..
        } => Some(*retry_after_sec),
        _ => None,
    };
    RpcError {
        code: error_code(&kind),
        message: message(&error),
        data: ErrorData {
            kind,
            detail: None,
            retry_after_sec,
        },
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&message(self))
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Acp(error) => Some(error),
            Self::Editor(error) => Some(error),
            Self::PaneState(error) => Some(error),
            Self::Vcs(error) => Some(error),
            Self::Web(error) => Some(error),
            Self::PortScan(error) => Some(error),
            Self::Supervisor(error) => Some(error),
            Self::Protocol(error) => Some(error),
            Self::WebSocket(error) => Some(error),
            Self::Utf8(error) => Some(error),
            Self::Address(error) => Some(error),
            Self::Join(error) => Some(error),
            _ => None,
        }
    }
}

impl From<soyeht_store::Error> for Error {
    fn from(error: soyeht_store::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<soyeht_acp::Error> for Error {
    fn from(error: soyeht_acp::Error) -> Self {
        Self::Acp(error)
    }
}

impl From<soyeht_pane_state::Error> for Error {
    fn from(error: soyeht_pane_state::Error) -> Self {
        Self::PaneState(error)
    }
}

impl From<soyeht_vcs::Error> for Error {
    fn from(error: soyeht_vcs::Error) -> Self {
        Self::Vcs(error)
    }
}

impl From<soyeht_web::Error> for Error {
    fn from(error: soyeht_web::Error) -> Self {
        Self::Web(error)
    }
}

impl From<crate::ports::PortScanError> for Error {
    fn from(error: crate::ports::PortScanError) -> Self {
        Self::PortScan(error)
    }
}

impl From<soyeht_ptyd::Error> for Error {
    fn from(error: soyeht_ptyd::Error) -> Self {
        Self::Supervisor(error)
    }
}

impl From<soyeht_protocol::Error> for Error {
    fn from(error: soyeht_protocol::Error) -> Self {
        Self::Protocol(error)
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(error: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WebSocket(error)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::Utf8(error)
    }
}

impl From<std::net::AddrParseError> for Error {
    fn from(error: std::net::AddrParseError) -> Self {
        Self::Address(error)
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::Join(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_git_is_its_own_kind_from_every_path_that_runs_it() {
        let wire_error = wire(Error::Vcs(soyeht_vcs::Error::GitUnavailable));
        assert_eq!(wire_error.data.kind, ErrorKind::GitUnavailable);
        assert_eq!(wire_error.message, soyeht_process::GIT_UNAVAILABLE_MESSAGE);
        let wire_error = wire(worktree_process(soyeht_process::Error::GitUnavailable));
        assert_eq!(wire_error.data.kind, ErrorKind::GitUnavailable);
        let wire_error = wire(worktree_process(soyeht_process::Error::TimedOut {
            program: "git".into(),
            after: std::time::Duration::from_secs(1),
        }));
        assert_eq!(wire_error.data.kind, ErrorKind::InternalError);
    }

    #[test]
    fn every_public_kind_is_assigned_at_the_single_wire_boundary() {
        let error = wire(Error::Supervisor(soyeht_ptyd::Error::Remote {
            kind: "stale_engine_generation".into(),
            message: "stale".into(),
        }));
        assert_eq!(error.data.kind, ErrorKind::StaleEngineGeneration);

        let error = wire(Error::Storage(soyeht_store::Error::IdempotencyConflict));
        assert_eq!(error.data.kind, ErrorKind::IdempotencyConflict);

        let error = wire(Error::Acp(soyeht_acp::Error::MissingSessionId));
        assert_eq!(error.data.kind, ErrorKind::HarnessUnavailable);

        assert_eq!(
            wire(stale_attempt("stale")).data.kind,
            ErrorKind::StaleAttempt
        );
        assert_eq!(
            wire(dependency_cycle("cycle")).data.kind,
            ErrorKind::DependencyCycle
        );
        assert_eq!(
            wire(dependency_open("open")).data.kind,
            ErrorKind::DependencyOpen
        );
        let limited = wire(rate_limited("wait", 17));
        assert_eq!(limited.data.kind, ErrorKind::RateLimited);
        assert_eq!(limited.data.retry_after_sec, Some(17));
        assert_eq!(
            wire(Error::Storage(soyeht_store::Error::TaskReservationConflict))
                .data
                .kind,
            ErrorKind::TaskHasOwner
        );
    }

    #[test]
    fn agent_poll_scope_uses_the_failing_boundary_instead_of_a_public_kind_allow_list() {
        assert!(!agent_poll_failure_is_global(&not_found("one pane")));
        assert!(!agent_poll_failure_is_global(&internal("one pane")));
        assert!(agent_poll_failure_is_global(&ptyd_unavailable(
            "shared supervisor"
        )));
        assert!(agent_poll_failure_is_global(&Error::Storage(
            soyeht_store::Error::IdempotencyConflict
        )));
        assert!(agent_poll_failure_is_global(&Error::Io(io::Error::from(
            io::ErrorKind::StorageFull
        ))));
    }

    #[test]
    fn a_failure_logs_its_kind_and_an_io_error_its_io_kind_never_its_text() {
        use soyeht_platform::log::Failure as _;
        let missing = Error::Io(io::Error::new(io::ErrorKind::NotFound, "/private/path"));
        assert_eq!(missing.failure_kind(), "io:NotFound");
        assert_eq!(not_found("a pane").failure_kind(), "not_found");
        assert_eq!(internal("/private/path").failure_kind(), "internal_error");
    }
}
