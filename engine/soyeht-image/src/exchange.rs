//! One request of an adapter, sent by the runner's `curl` (`soyeht_process::curl`), and what its
//! answer means: a success, or the error that status or failure stands for.

use crate::error::{Error, Result};
use serde_json::Value;
use soyeht_platform::secret::Secret;
use soyeht_process::curl::{self, Invalid, Request, SendError};
use std::time::Duration;

/// The most an answer may hold: an image as base64 inside a JSON document.
const MAX_RESPONSE_BYTES: usize = 48 * 1024 * 1024;
/// Longest text of the service's own error message that is kept.
const REASON_CHARS: usize = 200;
/// `curl`'s exit code for its own timeout.
const CURL_TIMED_OUT: i32 = 28;

pub(crate) fn invalid(_: Invalid) -> Error {
    Error::InvalidRequest("a value curl cannot be given")
}

/// A new request to `url`.
pub(crate) fn request(url: &str, https_only: bool) -> Result<Request> {
    let request = Request::new(url).map_err(invalid)?;
    Ok(if https_only {
        request.https_only()
    } else {
        request
    })
}

/// Sends `request` and returns the body of a success. `key` is the key the request carries, so
/// that the service's own words never repeat it.
pub(crate) fn send(request: Request, key: Option<&Secret>, timeout: Duration) -> Result<Vec<u8>> {
    let response = request
        .send(&curl::system_program(), timeout, MAX_RESPONSE_BYTES)
        .map_err(|error| match error {
            SendError::Exit(Some(CURL_TIMED_OUT)) => Error::TimedOut,
            SendError::Run(error) if error.timed_out() => Error::TimedOut,
            SendError::Run(_) | SendError::Exit(_) | SendError::Malformed => Error::Unreachable,
        })?;
    match response.status {
        200..=299 => Ok(response.body),
        401 | 403 => Err(Error::Unauthorized),
        429 => Err(Error::RateLimited),
        status => Err(Error::Provider {
            status,
            reason: reason(&response.body, key),
        }),
    }
}

/// [`send`], and the body read as JSON.
pub(crate) fn send_json(
    request: Request,
    key: Option<&Secret>,
    timeout: Duration,
) -> Result<Value> {
    serde_json::from_slice(&send(request, key, timeout)?)
        .map_err(|_| Error::BadResponse("not JSON"))
}

/// The service's own short explanation, from `{"error":{"message":"..."}}` or `{"error":"..."}`,
/// with anything unprintable dropped and the key, if the service repeated it, taken out.
fn reason(body: &[u8], key: Option<&Secret>) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let error = value.get("error")?;
    let mut text = error.get("message").unwrap_or(error).as_str()?.to_owned();
    if let Some(key) = key.map(Secret::expose).filter(|key| !key.is_empty()) {
        text = text.replace(key, "[key]");
    }
    let text: String = text
        .chars()
        .filter(|character| !character.is_control())
        .take(REASON_CHARS)
        .collect();
    Some(text).filter(|text| !text.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_reason_is_short_printable_and_never_holds_the_key() {
        let long = "x".repeat(500);
        let body = format!(r#"{{"error":{{"message":"a\u0007{long}"}}}}"#);
        assert_eq!(
            reason(body.as_bytes(), None).unwrap().chars().count(),
            REASON_CHARS
        );
        assert_eq!(
            reason(br#"{"error":"plain"}"#, None).as_deref(),
            Some("plain")
        );
        assert_eq!(reason(b"not json", None), None);
        let key = Secret::new("sk-abc123");
        let echoed = br#"{"error":{"message":"Incorrect key sk-abc123 provided"}}"#;
        assert_eq!(
            reason(echoed, Some(&key)).as_deref(),
            Some("Incorrect key [key] provided")
        );
    }
}
