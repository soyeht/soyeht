//! Bounded child processes for the usage host: an HTTPS request, `security` for the macOS
//! keychain and newline-delimited JSON-RPC for the Codex app server.
//!
//! Every program the host starts, here and in `terminal.rs`, begins from [`harden`], except the
//! request: a probe is background work, so it must never open a browser, show a prompt or wait for
//! a person. The request is built and run by `soyeht_process::curl`, the one place that builds a
//! command line for the system's `curl` (so a `.curlrc` of the person, such as a `proxy`, does not
//! apply to it; the proxy environment variables still do); `security` runs through
//! `soyeht_process::run`. Both have a deadline and a bounded output; the JSON-RPC child is
//! interactive and keeps its own deadline. Credentials reach `curl` on standard input as
//! configuration, never on its command line, and no response body or error text is logged.

use crate::streak::{Streak, first_failure, recovered};
use soyeht_platform::{
    log::{self, Failure as _},
    process::end_child,
};
use soyeht_process::curl;
use soyeht_protocol::limits;
use soyeht_usage::host::{HttpRequest, HttpResult, RpcRequest, RpcRun, Secret, SecretRead};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

pub(super) const SECURITY: &str = "/usr/bin/security";
/// A keychain read can wait on the user's answer to the macOS prompt.
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(60);
/// `security` exits with errSecItemNotFound when the item does not exist.
const KEYCHAIN_ITEM_NOT_FOUND: i32 = 44;
/// Redirects a usage request follows, all of them to https.
const MAX_REDIRECTS: u32 = 3;

/// The failures below repeat on every refresh of the usage rings, so each keeps a streak: the
/// first failure and each change of kind are logged, and a success ends the streak.
static CURL_RUN: Mutex<Streak> = Mutex::new(Streak::once());
static RPC_SPAWN: Mutex<Streak> = Mutex::new(Streak::once());
static RPC_WRITE: Mutex<Streak> = Mutex::new(Streak::once());

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
    timeout: Duration,
) -> soyeht_process::Result<soyeht_process::Output> {
    let mut command = Command::new(program);
    harden(&mut command);
    command.args(args);
    soyeht_process::run(
        command,
        soyeht_process::Limits::new(timeout, limits::JSON_BYTES),
    )
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
    let Ok(output) = run_bounded(program, &args, KEYCHAIN_TIMEOUT) else {
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

/// An HTTPS GET through the runner's `curl` (`soyeht_process::curl`): credential headers travel
/// in the configuration `curl` reads from standard input, never on its command line.
pub(super) fn http(program: &Path, request: &HttpRequest) -> HttpResult {
    let Ok(call) = http_call(request) else {
        return HttpResult::TransportFailed;
    };
    let timeout = request.timeout.max(Duration::from_secs(1));
    match call.send(program, timeout, limits::JSON_BYTES) {
        Ok(response) => {
            recovered(&CURL_RUN);
            HttpResult::Response {
                status: response.status,
                retry_after_sec: response.retry_after.as_deref().and_then(retry_after),
                body: response.body,
            }
        }
        // Any other exit is a transport failure that curl reports on its own.
        Err(curl::SendError::Exit(_) | curl::SendError::Malformed) => HttpResult::TransportFailed,
        Err(curl::SendError::Run(error)) => {
            if first_failure(&CURL_RUN, &error.failure_kind()) {
                log::failure("usage.curl_failed", &error);
            }
            HttpResult::TransportFailed
        }
    }
}

fn http_call(request: &HttpRequest) -> Result<curl::Request, curl::Invalid> {
    let mut call = curl::Request::new(&request.url)?
        .https_only()
        .follow_redirects(MAX_REDIRECTS);
    for (name, value) in &request.headers {
        call = call.header(name, value)?;
    }
    for (name, value) in &request.secret_headers {
        call = call.secret_header(name, value)?;
    }
    Ok(call)
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
        Err(error) => {
            if first_failure(&RPC_SPAWN, &error.failure_kind()) {
                log::failure("usage.rpc_spawn_failed", &error);
            }
            return RpcRun::Failed;
        }
    };
    recovered(&RPC_SPAWN);
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
                if first_failure(&RPC_WRITE, &error.failure_kind()) {
                    log::failure("usage.rpc_write_failed", &error);
                }
                return None;
            }
            if let Some(id) = call.id {
                results.push(response_with_id(&receiver, id, deadline)?);
            }
        }
        recovered(&RPC_WRITE);
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
    fn retry_after_is_read_as_seconds_or_a_date_and_capped() {
        assert_eq!(retry_after("17"), Some(17));
        assert_eq!(retry_after("99999"), Some(1_800));
        assert_eq!(retry_after("not a time"), None);
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
