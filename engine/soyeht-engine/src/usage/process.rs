//! Bounded child processes for the usage host: `curl` for HTTPS, `security` for the macOS
//! keychain and newline-delimited JSON-RPC for the Codex app server.
//!
//! Every process gets a deadline and a bounded output. Credentials reach `curl` on standard
//! input as configuration, never on its command line, and no response body or error text is
//! logged.

use soyeht_protocol::limits;
use soyeht_usage::host::{HttpRequest, HttpResult, RpcRequest, RpcRun, Secret, SecretRead};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
pub(super) const CURL: &str = "/usr/bin/curl";
pub(super) const SECURITY: &str = "/usr/bin/security";
/// A keychain read can wait on the user's answer to the macOS prompt.
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(60);
/// `security` exits with errSecItemNotFound when the item does not exist.
const KEYCHAIN_ITEM_NOT_FOUND: i32 = 44;
const STATUS_MARKER: &[u8] = b"\nSOYEHT_STATUS:";

pub(super) struct Output {
    pub code: Option<i32>,
    pub bytes: Vec<u8>,
}

/// Runs `program` with `input` on standard input, returning its standard output, or `None`
/// when it cannot start, outgrows the bound or misses the deadline.
pub(super) fn run_bounded(
    program: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    timeout: Duration,
) -> Option<Output> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let mut child = command.spawn().ok()?;
    if let Some(bytes) = input {
        let written = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(bytes).is_ok());
        if !written {
            stop_child(&mut child);
            return None;
        }
    }
    let Some(stdout) = child.stdout.take() else {
        stop_child(&mut child);
        return None;
    };
    let reader = thread::spawn(move || read_bounded(stdout));
    let code = wait_child(&mut child, Instant::now() + timeout)?;
    let bytes = reader.join().ok()??;
    Some(Output { code, bytes })
}

pub(super) fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_bounded(mut reader: impl Read) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = reader.read(&mut chunk).ok()?;
        if count == 0 {
            return Some(output);
        }
        if output.len().saturating_add(count) > limits::JSON_BYTES {
            return None;
        }
        output.extend_from_slice(&chunk[..count]);
    }
}

/// The exit code, `Some(None)` for a signal, or `None` past the deadline.
fn wait_child(child: &mut Child, deadline: Instant) -> Option<Option<i32>> {
    loop {
        if let Some(status) = child.try_wait().ok()? {
            return Some(status.code());
        }
        if Instant::now() >= deadline {
            stop_child(child);
            return None;
        }
        thread::sleep(POLL_INTERVAL);
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
    let Some(output) = run_bounded(program, &args, None, KEYCHAIN_TIMEOUT) else {
        return SecretRead::Denied;
    };
    match output.code {
        Some(0) => {
            let text = String::from_utf8(output.bytes).unwrap_or_default();
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
    match run_bounded(program, &args, Some(&config), timeout) {
        Some(output) if output.code == Some(0) => parse_http_output(output.bytes),
        _ => HttpResult::TransportFailed,
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
    let spawned = Command::new(program)
        .args(&request.args)
        .env("PATH", path_env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RpcRun::ToolNotFound;
        }
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
            write_json_line(&mut stdin, &message)?;
            if let Some(id) = call.id {
                results.push(response_with_id(&receiver, id, deadline)?);
            }
        }
        Some(results)
    })();
    stop_child(&mut child);
    results.map_or(RpcRun::Failed, RpcRun::Results)
}

fn write_json_line(stdin: &mut ChildStdin, value: &serde_json::Value) -> Option<()> {
    serde_json::to_writer(&mut *stdin, value).ok()?;
    stdin.write_all(b"\n").ok()?;
    stdin.flush().ok()
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

/// The `result` of the response with `id`, skipping notifications and other responses.
fn response_with_id(
    receiver: &Receiver<Line>,
    id: u64,
    deadline: Instant,
) -> Option<serde_json::Value> {
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        match receiver.recv_timeout(remaining).ok()? {
            Line::Bytes(bytes) => {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                if value["id"] == id {
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
}
