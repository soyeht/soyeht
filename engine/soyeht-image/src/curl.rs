//! One HTTP request through `curl`, run by the bounded runner (every external command goes
//! through `soyeht_process::run`: a deadline, a byte budget, no child left behind).
//!
//! Everything about the request except flags that hold no value of the person's goes in the
//! configuration `curl` reads from standard input: the address, the key header, the form fields
//! and the body. So a key is never on a command line, where another user's `ps` would see it.

use crate::{
    error::{Error, Result},
    vault::Secret,
};
use serde_json::Value;
use soyeht_process::Limits;
use std::{path::Path, process::Command, time::Duration};

/// The most a response may hold: an image as base64 inside a JSON document.
const MAX_RESPONSE_BYTES: usize = 48 * 1024 * 1024;
const CONNECT_SECONDS: &str = "10";
const STATUS_MARKER: &[u8] = b"\nSOYEHT_STATUS:";
/// Longest text of the service's own error message that is kept.
const REASON_CHARS: usize = 200;

/// What came back: the status and the body.
pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    /// The body when the status is a success; otherwise the error that status means.
    pub(crate) fn success(self) -> Result<Vec<u8>> {
        match self.status {
            200..=299 => Ok(self.body),
            401 | 403 => Err(Error::Unauthorized),
            429 => Err(Error::RateLimited),
            status => Err(Error::Provider {
                status,
                reason: reason(&self.body),
            }),
        }
    }
}

impl Response {
    /// The JSON document of a successful answer.
    pub(crate) fn json(self) -> Result<Value> {
        serde_json::from_slice(&self.success()?).map_err(|_| Error::BadResponse("not JSON"))
    }
}

/// The service's own short explanation, from `{"error":{"message":"..."}}` or
/// `{"error":"..."}`, with anything unprintable dropped.
fn reason(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let error = value.get("error")?;
    let text = error.get("message").unwrap_or(error).as_str()?;
    let text: String = text
        .chars()
        .filter(|character| !character.is_control())
        .take(REASON_CHARS)
        .collect();
    Some(text).filter(|text| !text.trim().is_empty())
}

/// One request being built.
pub(crate) struct Request {
    config: Vec<u8>,
    https_only: bool,
}

impl Request {
    /// `https_only` refuses a plain-http address. No redirect is followed.
    pub(crate) fn new(url: &str, https_only: bool) -> Result<Self> {
        let mut request = Self {
            config: Vec::new(),
            https_only,
        };
        request.line("url", url)?;
        Ok(request)
    }

    /// One `name = "value"` line, escaped as `curl` reads them.
    fn line(&mut self, name: &str, value: &str) -> Result<()> {
        self.config.extend_from_slice(name.as_bytes());
        self.config.extend_from_slice(b" = \"");
        for character in value.chars() {
            match character {
                '\\' => self.config.extend_from_slice(b"\\\\"),
                '"' => self.config.extend_from_slice(b"\\\""),
                '\n' => self.config.extend_from_slice(b"\\n"),
                '\r' => self.config.extend_from_slice(b"\\r"),
                '\t' => self.config.extend_from_slice(b"\\t"),
                other if other.is_control() => {
                    return Err(Error::InvalidRequest("a control character"));
                }
                other => {
                    let mut buffer = [0; 4];
                    self.config
                        .extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
                }
            }
        }
        self.config.extend_from_slice(b"\"\n");
        Ok(())
    }

    pub(crate) fn bearer(mut self, key: &Secret) -> Result<Self> {
        self.line("header", &format!("Authorization: Bearer {}", key.expose()))?;
        Ok(self)
    }

    /// A text field of a multipart form.
    pub(crate) fn field(mut self, name: &str, value: &str) -> Result<Self> {
        self.line("form-string", &format!("{name}={value}"))?;
        Ok(self)
    }

    /// A PNG file as a field of a multipart form, named `as_name` for the receiver.
    pub(crate) fn png_field(mut self, name: &str, file: &Path, as_name: &str) -> Result<Self> {
        let path = file
            .to_str()
            .filter(|path| !path.contains(['"', ';']))
            .ok_or(Error::InvalidRequest("a path curl cannot name"))?;
        self.line(
            "form",
            &format!("{name}=@\"{path}\";filename=\"{as_name}\";type=image/png"),
        )?;
        Ok(self)
    }

    /// A JSON body.
    pub(crate) fn json(mut self, body: &Value) -> Result<Self> {
        self.line("header", "Content-Type: application/json")?;
        self.line("data-binary", &body.to_string())?;
        Ok(self)
    }

    /// Sends the request and waits at most `timeout`.
    pub(crate) fn send(self, timeout: Duration) -> Result<Response> {
        let protocols = if self.https_only {
            "=https"
        } else {
            "=http,https"
        };
        let seconds = timeout.as_secs().max(1).to_string();
        let mut command = Command::new(program());
        command.args([
            "--config",
            "-",
            "--silent",
            "--max-time",
            &seconds,
            "--connect-timeout",
            CONNECT_SECONDS,
            "--proto",
            protocols,
            "--write-out",
            "\nSOYEHT_STATUS:%{http_code}\n",
        ]);
        let limits = Limits::new(timeout + Duration::from_secs(5), MAX_RESPONSE_BYTES);
        let output =
            soyeht_process::run_with_input(command, self.config, limits).map_err(Error::Command)?;
        if output.status.code() != Some(0) {
            return Err(Error::Unreachable);
        }
        parse(output.stdout)
    }
}

/// The system's `curl`; on a system without one in the usual place, the one on the search path
/// (a NixOS machine keeps none in `/usr/bin`).
#[cfg(unix)]
fn program() -> &'static str {
    const SYSTEM_CURL: &str = "/usr/bin/curl";
    if Path::new(SYSTEM_CURL).exists() {
        SYSTEM_CURL
    } else {
        "curl"
    }
}

/// Windows ships `curl.exe` in its system folder, which is on the search path.
#[cfg(windows)]
fn program() -> &'static str {
    "curl.exe"
}

fn parse(mut output: Vec<u8>) -> Result<Response> {
    let at = output
        .windows(STATUS_MARKER.len())
        .rposition(|window| window == STATUS_MARKER)
        .ok_or(Error::Unreachable)?;
    let status = String::from_utf8_lossy(&output[at + STATUS_MARKER.len()..])
        .trim()
        .parse::<u16>()
        .map_err(|_| Error::Unreachable)?;
    output.truncate(at);
    if status == 0 {
        return Err(Error::Unreachable);
    }
    Ok(Response {
        status,
        body: output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(request: &Request) -> String {
        String::from_utf8(request.config.clone()).unwrap()
    }

    #[test]
    fn a_value_is_escaped_the_way_curl_reads_its_configuration() {
        let request = Request::new("https://example.test/a", true)
            .unwrap()
            .field("prompt", "say \"hi\"\\\nnew line\ttab")
            .unwrap();
        assert!(text(&request).contains(r#"form-string = "prompt=say \"hi\"\\\nnew line\ttab""#));
    }

    #[test]
    fn a_control_character_cannot_start_a_new_option() {
        let refused = Request::new("https://example.test/a", true)
            .unwrap()
            .field("prompt", "x\u{0}\u{1b}[0m");
        assert!(matches!(refused, Err(Error::InvalidRequest(_))));
    }

    #[test]
    fn the_status_marker_ends_the_body_even_when_the_body_holds_one() {
        let parsed = parse(b"{\"a\":1}\nSOYEHT_STATUS:200\n".to_vec()).unwrap();
        assert_eq!(
            (parsed.status, parsed.body.as_slice()),
            (200, &b"{\"a\":1}"[..])
        );
        assert!(matches!(
            parse(b"no marker".to_vec()),
            Err(Error::Unreachable)
        ));
        assert!(matches!(
            parse(b"\nSOYEHT_STATUS:000\n".to_vec()),
            Err(Error::Unreachable)
        ));
    }

    #[test]
    fn a_service_reason_is_short_and_printable() {
        let long = "x".repeat(500);
        let body = format!(r#"{{"error":{{"message":"a\u0007{long}"}}}}"#);
        assert_eq!(
            reason(body.as_bytes()).unwrap().chars().count(),
            REASON_CHARS
        );
        assert_eq!(reason(br#"{"error":"plain"}"#).as_deref(), Some("plain"));
        assert_eq!(reason(b"not json"), None);
    }
}
