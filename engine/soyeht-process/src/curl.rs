//! One HTTP request through `curl`, run by the bounded runner: the one place that builds a `curl`
//! command line, so every caller gets the same guarantees.
//!
//! - **The system's `curl`, started with `--disable`** as its first argument, so no `.curlrc` of
//!   the person (a `proxy`, an `insecure`, a `location`) changes what a request does, and with
//!   `--globoff`, so `{a,b}` or `[1-9]` in an address is never expanded into several requests.
//! - **Credentials stay off the command line.** Headers, form fields and the body go in the
//!   configuration `curl` reads on standard input; only the address (which holds no secret) is an
//!   argument.
//! - **No redirect is followed** unless the caller asks, and then only as many as it names.
//! - A [`Secret`] never prints.

use crate::{Error, Limits, run_with_input};
use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

const STATUS_MARKER: &[u8] = b"\nSOYEHT_STATUS:";
const STATUS_PREFIX: &str = "SOYEHT_STATUS:";
const RETRY_PREFIX: &str = "SOYEHT_RETRY:";
const WRITE_OUT: &str = "\nSOYEHT_STATUS:%{http_code}\nSOYEHT_RETRY:%header{retry-after}\n";
const CONNECT_SECONDS: &str = "10";
/// How long past the request's own limit the runner waits before it kills `curl`.
const KILL_GRACE: Duration = Duration::from_secs(5);
/// Upper bound for one token or cookie value.
pub const MAX_SECRET_BYTES: usize = 16 * 1024;

/// A credential or cookie value. It never prints, so it cannot reach a log by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Printable ASCII without whitespace or quotes, within the size bound: safe to place in a
    /// header without escaping.
    pub fn is_usable(&self) -> bool {
        let value = self.0.as_bytes();
        !value.is_empty()
            && value.len() <= MAX_SECRET_BYTES
            && value
                .iter()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

/// The `curl` the system ships, never one found on a search path a program of the person's could
/// have changed: on Unix the first of the system's own locations that exists (the last is a NixOS
/// machine's system profile, which keeps nothing in `/usr/bin`); on Windows `curl.exe` of the
/// system folder.
#[cfg(unix)]
pub fn system_program() -> PathBuf {
    const LOCATIONS: [&str; 3] = [
        "/usr/bin/curl",
        "/bin/curl",
        "/run/current-system/sw/bin/curl",
    ];
    LOCATIONS
        .iter()
        .map(Path::new)
        .find(|path| path.exists())
        .unwrap_or(Path::new(LOCATIONS[0]))
        .to_path_buf()
}

#[cfg(windows)]
pub fn system_program() -> PathBuf {
    PathBuf::from("curl.exe")
}

/// A value that cannot be written into a request, with a fixed reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invalid(pub &'static str);

impl fmt::Display for Invalid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// Why a request got no answer.
#[derive(Debug)]
pub enum SendError {
    /// `curl` did not start, did not finish within its deadline or wrote too much.
    Run(Error),
    /// `curl` exited with this code (28 is its own timeout).
    Exit(Option<i32>),
    /// `curl` finished but its output has no status.
    Malformed,
}

/// What came back.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    /// The `Retry-After` header as the service wrote it.
    pub retry_after: Option<String>,
    pub body: Vec<u8>,
}

/// One request being built.
pub struct Request {
    url: String,
    config: Vec<u8>,
    https_only: bool,
    redirects: u32,
}

impl Request {
    /// A request to `url`, an `http` or `https` address with no whitespace or control character.
    pub fn new(url: &str) -> Result<Self, Invalid> {
        let scheme = url.starts_with("https://") || url.starts_with("http://");
        if !scheme || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(Invalid("an address that is not http or https"));
        }
        Ok(Self {
            url: url.to_owned(),
            config: Vec::new(),
            https_only: false,
            redirects: 0,
        })
    }

    /// Refuses an `http` address, and an `http` redirect.
    pub fn https_only(mut self) -> Self {
        self.https_only = true;
        self
    }

    /// Follows at most `limit` redirects (none by default).
    pub fn follow_redirects(mut self, limit: u32) -> Self {
        self.redirects = limit;
        self
    }

    /// One `name = "value"` line, escaped as `curl` reads its configuration.
    fn line(&mut self, name: &str, value: &str) -> Result<(), Invalid> {
        let mut text = String::with_capacity(name.len() + value.len() + 8);
        text.push_str(name);
        text.push_str(" = \"");
        for character in value.chars() {
            match character {
                '\\' => text.push_str("\\\\"),
                '"' => text.push_str("\\\""),
                '\n' => text.push_str("\\n"),
                '\r' => text.push_str("\\r"),
                '\t' => text.push_str("\\t"),
                other if other.is_control() => return Err(Invalid("a control character")),
                other => text.push(other),
            }
        }
        text.push_str("\"\n");
        self.config.extend_from_slice(text.as_bytes());
        Ok(())
    }

    fn named_header(mut self, name: &str, value: &str) -> Result<Self, Invalid> {
        let plain =
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !plain || value.is_empty() {
            return Err(Invalid("a header with no name or no value"));
        }
        self.line("header", &format!("{name}: {value}"))?;
        Ok(self)
    }

    /// A header that holds no credential.
    pub fn header(self, name: &str, value: &str) -> Result<Self, Invalid> {
        self.named_header(name, value)
    }

    /// A header that carries a credential.
    pub fn secret_header(self, name: &str, value: &Secret) -> Result<Self, Invalid> {
        self.named_header(name, value.expose())
    }

    /// `Authorization: Bearer <key>`.
    pub fn bearer(self, key: &Secret) -> Result<Self, Invalid> {
        self.named_header("Authorization", &format!("Bearer {}", key.expose()))
    }

    /// A text field of a multipart form; its value is never read as a file name.
    pub fn form_text(mut self, name: &str, value: &str) -> Result<Self, Invalid> {
        self.line("form-string", &format!("{name}={value}"))?;
        Ok(self)
    }

    /// A file as a field of a multipart form, shown to the receiver as `as_name`. The path goes
    /// through two readings, the form's (which takes `\` and `"` inside quotes as escapes) and the
    /// configuration's, so each is escaped for its own.
    pub fn form_file(
        mut self,
        name: &str,
        file: &Path,
        as_name: &str,
        content_type: &str,
    ) -> Result<Self, Invalid> {
        let quoted = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
        let path = file.to_str().ok_or(Invalid("a path that is not text"))?;
        self.line(
            "form",
            &format!(
                "{name}=@\"{}\";filename=\"{}\";type={content_type}",
                quoted(path),
                quoted(as_name)
            ),
        )?;
        Ok(self)
    }

    /// A JSON body.
    pub fn json_body(mut self, body: &str) -> Result<Self, Invalid> {
        self.line("header", "Content-Type: application/json")?;
        self.line("data-binary", body)?;
        Ok(self)
    }

    /// The command line: `--disable` first, the address last.
    fn arguments(&self, timeout: Duration) -> Vec<String> {
        let protocols = if self.https_only {
            "=https"
        } else {
            "=http,https"
        };
        let mut arguments = [
            "--disable",
            "--config",
            "-",
            "--silent",
            "--globoff",
            "--connect-timeout",
            CONNECT_SECONDS,
            "--proto",
            protocols,
            "--proto-redir",
            protocols,
            "--max-redirs",
        ]
        .map(str::to_owned)
        .to_vec();
        arguments.push(self.redirects.to_string());
        if self.redirects > 0 {
            arguments.push("--location".to_owned());
        }
        arguments.extend([
            "--max-time".to_owned(),
            timeout.as_secs().max(1).to_string(),
            "--write-out".to_owned(),
            WRITE_OUT.to_owned(),
            self.url.clone(),
        ]);
        arguments
    }

    /// Sends the request with `program` and waits at most `timeout`, keeping at most `max_bytes` of
    /// the answer.
    pub fn send(
        self,
        program: &Path,
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<Response, SendError> {
        let mut command = std::process::Command::new(program);
        command.args(self.arguments(timeout));
        let limits = Limits::new(timeout + KILL_GRACE, max_bytes);
        let output = run_with_input(command, self.config, limits).map_err(SendError::Run)?;
        if output.status.code() != Some(0) {
            return Err(SendError::Exit(output.status.code()));
        }
        parse(output.stdout)
    }
}

fn parse(mut output: Vec<u8>) -> Result<Response, SendError> {
    let at = output
        .windows(STATUS_MARKER.len())
        .rposition(|window| window == STATUS_MARKER)
        .ok_or(SendError::Malformed)?;
    let trailer = String::from_utf8_lossy(&output.split_off(at + 1)).into_owned();
    output.truncate(at);
    let field = |prefix: &str| {
        trailer
            .lines()
            .find_map(|line| line.strip_prefix(prefix).map(str::trim))
            .filter(|value| !value.is_empty())
    };
    match field(STATUS_PREFIX).and_then(|value| value.parse::<u16>().ok()) {
        Some(0) | None => Err(SendError::Malformed),
        Some(status) => Ok(Response {
            status,
            retry_after: field(RETRY_PREFIX).map(str::to_owned),
            body: output,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(request: &Request) -> String {
        String::from_utf8(request.config.clone()).unwrap()
    }

    fn request() -> Request {
        Request::new("https://example.test/a").unwrap()
    }

    #[test]
    fn the_command_line_disables_the_rc_file_first_and_holds_no_value_of_the_caller() {
        let key = Secret::new("sk-secret");
        let request = request()
            .bearer(&key)
            .unwrap()
            .form_text("prompt", "say hi")
            .unwrap()
            .json_body("{\"a\":1}")
            .unwrap();
        let arguments = request.arguments(Duration::from_secs(30));
        assert_eq!(arguments[0], "--disable");
        assert!(arguments.contains(&"--globoff".to_owned()));
        assert_eq!(
            arguments.last().map(String::as_str),
            Some("https://example.test/a")
        );
        let joined = arguments.join(" ");
        for hidden in ["sk-secret", "say hi", "Bearer", "\"a\""] {
            assert!(!joined.contains(hidden), "{hidden}");
        }
        assert!(text(&request).contains("Authorization: Bearer sk-secret"));
    }

    #[test]
    fn a_redirect_is_followed_only_when_asked_and_never_to_http_from_an_https_only_request() {
        let none = request().https_only().arguments(Duration::from_secs(5));
        let at = none.iter().position(|a| a == "--max-redirs").unwrap();
        assert_eq!(none[at + 1], "0");
        assert!(!none.contains(&"--location".to_owned()));
        assert!(none.contains(&"=https".to_owned()));
        let some = request()
            .follow_redirects(3)
            .arguments(Duration::from_secs(5));
        assert!(some.contains(&"--location".to_owned()));
    }

    #[test]
    fn an_address_that_is_not_http_or_holds_whitespace_is_refused() {
        for bad in [
            "ftp://x",
            "-o /etc/passwd",
            "https://a b",
            "https://a\nb",
            "",
        ] {
            assert!(Request::new(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_value_is_escaped_the_way_curl_reads_its_configuration() {
        let request = request()
            .form_text("prompt", "say \"hi\"\\\nnew line\ttab")
            .unwrap();
        assert!(text(&request).contains(r#"form-string = "prompt=say \"hi\"\\\nnew line\ttab""#));
    }

    #[test]
    fn a_control_character_cannot_start_a_new_option() {
        assert!(request().form_text("prompt", "x\u{0}\u{1b}[0m").is_err());
    }

    #[test]
    fn a_header_cannot_be_split_into_two_lines_or_named_with_a_colon() {
        let request = request().header("X-A", "a\nHost: evil.test").unwrap();
        // The newline is escaped inside one configuration value: curl reads one header.
        assert_eq!(text(&request).lines().count(), 1);
        assert!(self::request().header("X: y", "v").is_err());
        assert!(self::request().header("", "v").is_err());
    }

    #[test]
    fn a_path_is_escaped_for_the_form_and_then_for_the_configuration() {
        let unc = request()
            .form_file(
                "image",
                Path::new(r"\\server\share\face.png"),
                "a.png",
                "image/png",
            )
            .unwrap();
        // `\\server` for the form is `\\\\server` for the configuration.
        assert!(text(&unc).contains(r#"image=@\"\\\\\\\\server\\\\share\\\\face.png\""#));
        let quote = request()
            .form_file("image", Path::new("we\"ird;name.png"), "a.png", "image/png")
            .unwrap();
        assert!(text(&quote).contains(r#"@\"we\\\"ird;name.png\""#));
    }

    #[test]
    fn the_status_marker_ends_the_body_even_when_the_body_holds_one() {
        let parsed = parse(b"{\"a\":1}\nSOYEHT_STATUS:200\nSOYEHT_RETRY:7\n".to_vec()).unwrap();
        assert_eq!(
            (parsed.status, parsed.body.as_slice()),
            (200, &b"{\"a\":1}"[..])
        );
        assert_eq!(parsed.retry_after.as_deref(), Some("7"));
        assert!(matches!(
            parse(b"no marker".to_vec()),
            Err(SendError::Malformed)
        ));
        assert!(matches!(
            parse(b"\nSOYEHT_STATUS:000\n".to_vec()),
            Err(SendError::Malformed)
        ));
    }

    #[test]
    fn a_secret_never_prints() {
        assert!(!format!("{:?}", Secret::new("sk-secret")).contains("secret"));
    }
}
