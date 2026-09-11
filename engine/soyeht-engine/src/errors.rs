use soyeht_protocol::{ErrorData, ErrorKind, RpcError, error_code};

pub type Result<T> = std::result::Result<T, RpcError>;

pub fn error(kind: ErrorKind, message: impl Into<String>) -> RpcError {
    RpcError {
        code: error_code(&kind),
        message: message.into(),
        data: ErrorData { kind, detail: None },
    }
}

pub fn storage(error_value: soyeht_store::database::Error) -> RpcError {
    use soyeht_store::database::Error;
    match error_value {
        Error::IdempotencyConflict => error(
            ErrorKind::IdempotencyConflict,
            "The idempotency key has different parameters",
        ),
        Error::CursorTooOld => error(ErrorKind::CursorTooOld, "Request a new snapshot"),
        Error::Invalid(message) => error(ErrorKind::InvalidParams, message),
        Error::DiskFull => error(ErrorKind::DiskFull, "Storage is full"),
        _ => error(ErrorKind::InternalError, "Storage operation failed"),
    }
}

pub fn io(error_value: std::io::Error) -> RpcError {
    if error_value.raw_os_error() == Some(libc::ENOSPC) {
        error(ErrorKind::DiskFull, "Storage is full")
    } else {
        error(ErrorKind::InternalError, "I/O operation failed")
    }
}
