use serde::Serialize;
use serde_json::Value;
use soyeht_protocol::{ErrorData, ErrorKind, RpcError, error_code};

pub type Result<T> = std::result::Result<T, RpcError>;

pub fn error(kind: ErrorKind, message: impl Into<String>) -> RpcError {
    RpcError {
        code: error_code(&kind),
        message: message.into(),
        data: ErrorData { kind, detail: None },
    }
}

pub fn invalid(message: impl Into<String>) -> RpcError {
    error(ErrorKind::InvalidParams, message)
}

pub fn internal(message: impl Into<String>) -> RpcError {
    error(ErrorKind::InternalError, message)
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
