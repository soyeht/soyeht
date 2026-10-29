//! Bounded child processes for the usage host: `curl` for HTTPS, `security` for the macOS
//! keychain and newline-delimited JSON-RPC for the Codex app server.
//!
//! Every program the host starts, here and in `terminal.rs`, begins from [`harden`]: a probe is
//! background work, so it must never open a browser, show a prompt or wait for a person.
//! `curl` and `security` run through `soyeht_process`, with a deadline and a bounded output;
//! the JSON-RPC child is interactive and keeps its own deadline. Credentials reach `curl` on standard
//! input as configuration, never on its command line, and no response body or error text is
//! logged.

use soyeht_platform::process::end_child;
use soyeht_protocol::limits;
use soyeht_usage::host::{HttpRequest, HttpResult, RpcRequest, RpcRun, Secret, SecretRead};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
pub(super) const CURL: &str = "/usr/bin/curl";
/// Windows ships `curl.exe` in its system folder, which is on the search path.
#[cfg(windows)]
pub(super) const CURL: &str = "curl.exe";
pub(super) const SECURITY: &str = "/usr/bin/security";
/// A keychain read can wait on the user's answer to the macOS prompt.
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(60);
/// `security` exits with errSecItemNotFound when the item does not exist.
const KEYCHAIN_ITEM_NOT_FOUND: i32 = 44;
const STATUS_MARKER: &[u8] = b"\nSOYEHT_STATUS:";

/// A program that opens nothing and exits at once. Windows has no stock program like it and its
/// openers do not read `BROWSER`, so nothing is set there.
#[cfg(unix)]
pub(super) const NO_BROWSER: &str = "/usr/bin/true";

/// The one rule for every command the usage host starts: `BROWSER`, which Unix opener scripts
/// and many command line tools read, names a program that does nothing. Standard input needs no
/// rule here: the bounded runner gives none, and the JSON-RPC child and the pseudo terminal get
/// a pipe that only the source's own conversation writes to. The sources add the real guards:
/// Claude Code is not started without a stored login, and its driver presses no key on a screen
/// it does not know.
#[cfg(unix)]
pub(super) fn harden(command: &mut Command) {
    command.env("BROWSER", NO_BROWSER);
}

#[cfg(windows)]
pub(super) fn harden(_command: &mut Command) {}

/// Runs `program` with `input` on standard input through the bounded runner: a deadline, the
/// usage output budget and no child left behind.
///
/// The failure is returned unchanged for the caller to name its own outcome. Nothing here
/// logs it: `curl` and `security` handle credentials, and the usage host reports only
/// whether a source was denied or unreachable, never why.
fn run_bounded(
    program: &Path,
    args: &[&str],
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> soyeht_process::Result<soyeht_process::Output> {
    let mut command = Command::new(program);
    harden(&mut command);
    command.args(args);
    let limits = soyeht_process::Limits::new(timeout, limits::JSON_BYTES);
    match input {
        Some(input) => soyeht_process::run_with_input(command, input, limits),
        None => soyeht_process::run(command, limits),
    }
}

/// A generic password from the login keychain through `/usr/bin/security`. The first read of
/// another application's item may show the macOS consent prompt.
pub(super) fn keychain_password(
    program: &Path,
    service: &str,
    account: Option<&str>,
) -> SecretRead {
    let mut args = vec!["find-generic-password", "-s", service];
    if let Some(account) = account {
        args.extend(["-a", account]);
    }
    args.push("-w");
    // A keychain that did not answer in time, or at all, is a read the person did not allow.
    let Ok(output) = run_bounded(program, &args, None, KEYCHAIN_TIMEOUT) else {
        return SecretRead::Denied;
    };
    match output.status.code() {
        Some(0) => {
            let text = String::from_utf8(output.stdout).unwrap_or_default();
            let secret = Secret::new(text.trim_end_matches(['\r', '\n']));
            if secret.expose().is_empty() {
                SecretRead::Missing
            } else {
                SecretRead::Found(secret)
            }
        }
        Some(KEYCHAIN_ITEM_NOT_FOUND) => SecretRead::Missing,
        _ => SecretRead::Denied,
    }
}

/// An HTTPS GET through `curl`. Credential headers travel in the configuration curl reads from
/// standard input.
pub(super) fn http(program: &Path, request: &HttpRequest) -> HttpResult {
    let mut config = Vec::new();
    for (name, value) in &request.secret_headers {
        // Printable ASCII without quotes or backslashes needs no escaping in curl's config.
        let quotable = value
            .expose()
            .bytes()
            .all(|byte| (byte.is_ascii_graphic() || byte == b' ') && !matches!(byte, b'"' | b'\\'));
        if value.expose().is_empty() || !quotable {
            return HttpResult::TransportFailed;
        }
        config.extend_from_slice(format!("header = \"{name}: ").as_bytes());
        config.extend_from_slice(value.expose().as_bytes());
        config.extend_from_slice(b"\"\n");
    }
    let max_time = request.timeout.as_secs().max(1).to_string();
    let mut args = vec![
        "--config",
        "-",
        "--silent",
        "--max-time",
        &max_time,
        "--connect-timeout",
        "10",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-redirs",
        "3",
        "--location",
        "--write-out",
        "\nSOYEHT_STATUS:%{http_code}\nSOYEHT_RETRY:%header{retry-after}\n",
    ];
    let headers = request
        .headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect::<Vec<_>>();
    for header in &headers {
        args.extend(["--header", header.as_str()]);
    }
    args.push(&request.url);
    let timeout = request.timeout + Duration::from_secs(5);
    match run_bounded(program, &args, Some(config), timeout) {
        Ok(output) if output.status.code() == Some(0) => parse_http_output(output.stdout),
        // ignored: any other exit, and every failure to run curl, is a transport failure.
        Ok(_) | Err(_) => HttpResult::TransportFailed,
    }
}

fn parse_http_output(mut output: Vec<u8>) -> HttpResult {
    let Some(at) = output
        .windows(STATUS_MARKER.len())
        .rposition(|window| window == STATUS_MARKER)
    else {
        return HttpResult::TransportFailed;
    };
    let metadata = String::from_utf8_lossy(&output.split_off(at + 1)).into_owned();
    output.truncate(at);
    let field = |prefix: &str| {
        metadata
            .lines()
            .find_map(|line| line.strip_prefix(prefix).map(str::trim))
            .filter(|value| !value.is_empty())
    };
    match field("SOYEHT_STATUS:").and_then(|value| value.parse::<u16>().ok()) {
        Some(0) | None => HttpResult::TransportFailed,
        Some(status) => HttpResult::Response {
            status,
            retry_after_sec: field("SOYEHT_RETRY:").and_then(retry_after),
            body: output,
        },
    }
}

/// `Retry-After` as seconds or an HTTP date, capped at the protocol's longest backoff.
fn retry_after(value: &str) -> Option<u32> {
    let seconds = value.parse::<i64>().ok().or_else(|| {
        chrono::DateTime::parse_from_rfc2822(value)
            .ok()
            .map(|date| date.timestamp() - chrono::Utc::now().timestamp())
    })?;
    u32::try_from(seconds.clamp(0, limits::USAGE_MAX_BACKOFF_SEC as i64)).ok()
}

enum Line {
    Bytes(Vec<u8>),
    End,
}

/// One JSON-RPC exchange with a child that speaks one JSON document per line.
pub(super) fn json_rpc(program: &Path, path_env: &str, request: &RpcRequest) -> RpcRun {
    let mut command = Command::new(program);
    harden(&mut command);
    command
        .args(&request.args)
        .env("PATH", path_env)
        // The conversation itself: the only input this child gets.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // The exchange is a conversation over the pipes, which `soyeht_process::run` does not
    // offer; the start still queues with every other.
    let Ok(spawned) = soyeht_process::starting("usage source", || command.spawn()) else {
        // Refused under the Core lock; `Failed` reaches the reading as an unavailable source.
        return RpcRun::Failed;
    };
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RpcRun::ToolNotFound;
        }
        // ignored: `Failed` reaches the reading as an unavailable source.
        Err(_) => return RpcRun::Failed,
    };
    let deadline = Instant::now() + request.timeout;
    let results = (|| {
        let mut stdin = child.stdin.take()?;
        let receiver = spawn_line_reader(child.stdout.take()?);
        let mut results = Vec::new();
        for call in &request.calls {
            let mut message =
                serde_json::json!({"jsonrpc": "2.0", "method": call.method, "params": call.params});
            if let Some(id) = call.id {
                message["id"] = id.into();
            }
            if let Err(error) = write_json_line(&mut stdin, &message) {
                soyeht_platform::log::failure("usage.rpc_write_failed", &error);
                return None;
            }
            if let Some(id) = call.id {
                results.push(response_with_id(&receiver, id, deadline)?);
            }
        }
        Some(results)
    })();
    end_child(&mut child);
    results.map_or(RpcRun::Failed, RpcRun::Results)
}

fn write_json_line(stdin: &mut ChildStdin, value: &serde_json::Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *stdin, value)?;
    stdin.write_all(b"\n")?;
    stdin.flush()
}

fn spawn_line_reader(stdout: impl Read + Send + 'static) -> Receiver<Line> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut bytes = Vec::new();
            let count = reader
                .by_ref()
                .take((limits::JSON_BYTES + 1) as u64)
                .read_until(b'\n', &mut bytes)
                .unwrap_or(0);
            if count == 0 || bytes.len() > limits::JSON_BYTES {
                // ignored: the receiver is gone once the exchange ended
                let _ = sender.send(Line::End);
                return;
            }
            if sender.send(Line::Bytes(bytes)).is_err() {
                return;
            }
        }
    });
    receiver
}

/// The `result` of the response with `id`, skipping notifications and other responses. An error
/// response gives `null`: the caller decides whether that call mattered.
fn response_with_id(
    receiver: &Receiver<Line>,
    id: u64,
    deadline: Instant,
) -> Option<serde_json::Value> {
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        // ignored: a timeout or a closed reader ends the exchange, and the caller reports it failed
        match receiver.recv_timeout(remaining).ok()? {
            Line::Bytes(bytes) => {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                if value["id"] == id {
                    if value.get("error").is_some() {
                        return Some(serde_json::Value::Null);
                    }
                    return value.get_mut("result").map(serde_json::Value::take);
                }
            }
            Line::End => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers(lines: &[&str]) -> Receiver<Line> {
        let (sender, receiver) = mpsc::channel();
        for line in lines {
            sender.send(Line::Bytes(line.as_bytes().to_vec())).unwrap();
        }
        receiver
    }

    #[test]
    fn an_error_answer_is_null_and_the_exchange_goes_on() {
        let receiver = answers(&[
            r#"{"jsonrpc":"2.0","method":"note","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32603,"message":"token expired"}}"#,
            r#"{"jsonrpc":"2.0","id":3,"result":{"rateLimits":{}}}"#,
        ]);
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(
            response_with_id(&receiver, 2, deadline),
            Some(serde_json::Value::Null)
        );
        assert_eq!(
            response_with_id(&receiver, 3, deadline),
            Some(serde_json::json!({"rateLimits": {}}))
        );
    }

    #[test]
    fn http_status_and_retry_after_are_split_from_the_body() {
        let output = b"{\"a\":1}\nSOYEHT_STATUS:429\nSOYEHT_RETRY:17\n".to_vec();
        assert_eq!(
            parse_http_output(output),
            HttpResult::Response {
                status: 429,
                retry_after_sec: Some(17),
                body: b"{\"a\":1}".to_vec()
            }
        );
        assert_eq!(
            parse_http_output(b"\nSOYEHT_STATUS:000\nSOYEHT_RETRY:\n".to_vec()),
            HttpResult::TransportFailed
        );
        assert_eq!(retry_after("99999"), Some(1_800));
    }

    #[cfg(unix)]
    #[test]
    fn every_probe_command_names_a_browser_that_does_nothing() {
        let mut command = Command::new("sh");
        harden(&mut command);
        command.args(["-c", "echo \"[$BROWSER]\""]);
        let limits = soyeht_process::Limits::new(Duration::from_secs(10), limits::JSON_BYTES);
        let output = soyeht_process::run(command, limits).unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains(&format!("[{NO_BROWSER}]")), "{text}");
    }
}
