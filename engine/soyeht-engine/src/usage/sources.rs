//! Bounded adapters for the verified local usage sources.

use soyeht_protocol::{UsageProvider, UsageSource, UsageUnavailableReason, limits};
use soyeht_usage::{SourceRead, UsageSourcePort};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const SOURCE_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const CURSOR_ENDPOINT: &str =
    "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";

#[derive(Clone)]
struct Commands {
    claude_cache: PathBuf,
    second_cli: PathBuf,
    keychain: PathBuf,
    http: PathBuf,
}

pub struct SystemSources {
    commands: Commands,
}

struct Output {
    success: bool,
    bytes: Vec<u8>,
}

enum Line {
    Bytes(Vec<u8>),
    Oversized,
    End,
}

impl Default for SystemSources {
    fn default() -> Self {
        Self {
            commands: default_commands(),
        }
    }
}

fn default_commands() -> Commands {
    #[cfg(debug_assertions)]
    if let Some(directory) = std::env::var_os("SOYEHT_TEST_USAGE_READER_DIR").map(PathBuf::from) {
        return Commands {
            claude_cache: directory.join("claude-cache.json"),
            second_cli: directory.join("second-source"),
            keychain: directory.join("keychain-source"),
            http: directory.join("http-source"),
        };
    }
    Commands {
        claude_cache: std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".claude.json"))
            .unwrap_or_else(|| PathBuf::from("/nonexistent/soyeht-claude-cache.json")),
        second_cli: "codex".into(),
        keychain: "/usr/bin/security".into(),
        http: "/usr/bin/curl".into(),
    }
}

impl UsageSourcePort for SystemSources {
    fn read(&mut self, provider: UsageProvider, source: UsageSource) -> SourceRead {
        match (provider, source) {
            (UsageProvider::Claude, UsageSource::ClaudeDesktopCache) => self.read_claude_cache(),
            (UsageProvider::Codex, UsageSource::CodexAppServer) => self.read_second_cli(),
            (UsageProvider::Cursor, UsageSource::CursorCurrentPeriod) => self.read_current_period(),
            _ => SourceRead::Unavailable(None),
        }
    }
}

impl SystemSources {
    fn read_claude_cache(&self) -> SourceRead {
        let Ok(metadata) = fs::metadata(&self.commands.claude_cache) else {
            return SourceRead::Unavailable(None);
        };
        let Ok(max_bytes) = u64::try_from(limits::JSON_BYTES) else {
            return SourceRead::Unavailable(None);
        };
        if metadata.len() > max_bytes {
            return SourceRead::Unavailable(None);
        }
        fs::read(&self.commands.claude_cache)
            .ok()
            .filter(|bytes| bytes.len() <= limits::JSON_BYTES)
            .map_or(SourceRead::Unavailable(None), SourceRead::Payload)
    }

    fn read_second_cli(&self) -> SourceRead {
        match app_server_read(&self.commands.second_cli) {
            Ok(bytes) => SourceRead::Payload(bytes),
            Err(reason) => SourceRead::Unavailable(Some(reason)),
        }
    }

    fn read_current_period(&self) -> SourceRead {
        let lookup = [
            "find-generic-password",
            "-s",
            "cursor-access-token",
            "-a",
            "cursor-user",
        ];
        if !run_bounded(&self.commands.keychain, &lookup, None).is_some_and(|o| o.success) {
            return SourceRead::LoginNeeded;
        }
        let reveal = [
            "find-generic-password",
            "-s",
            "cursor-access-token",
            "-a",
            "cursor-user",
            "-w",
        ];
        let Some(secret) = run_bounded(&self.commands.keychain, &reveal, None)
            .filter(|output| output.success)
            .map(|output| output.bytes)
        else {
            return SourceRead::NoAccess;
        };
        let token = trim_ascii(&secret);
        if !valid_token(token) {
            return SourceRead::Unavailable(None);
        }
        current_period_request(&self.commands.http, token)
    }

    #[cfg(test)]
    fn with_commands(commands: Commands) -> Self {
        Self { commands }
    }
}

fn current_period_request(program: &PathBuf, token: &[u8]) -> SourceRead {
    let mut config = b"header = \"Authorization: Bearer ".to_vec();
    config.extend_from_slice(token);
    config.extend_from_slice(b"\"\n");
    let args = [
        "--config",
        "-",
        "--silent",
        "--show-error",
        "--max-time",
        "10",
        "--connect-timeout",
        "5",
        "--proto",
        "=https",
        "--request",
        "POST",
        "--header",
        "Content-Type: application/json",
        "--header",
        "Connect-Protocol-Version: 1",
        "--data",
        "{}",
        "--write-out",
        "\nSOYEHT_STATUS:%{http_code}\nSOYEHT_RETRY:%header{retry-after}\n",
        CURSOR_ENDPOINT,
    ];
    let Some(output) = run_bounded(program, &args, Some(&config)) else {
        return SourceRead::NetworkError;
    };
    if !output.success {
        return SourceRead::NetworkError;
    }
    parse_http_output(output.bytes)
}

fn parse_http_output(mut output: Vec<u8>) -> SourceRead {
    let Some(status_at) = find_last(&output, b"\nSOYEHT_STATUS:") else {
        return SourceRead::Unavailable(None);
    };
    let metadata = output.split_off(status_at + 1);
    output.truncate(status_at);
    let text = String::from_utf8_lossy(&metadata);
    let status = field(&text, "SOYEHT_STATUS:").and_then(|value| value.parse::<u16>().ok());
    match status {
        Some(200) => SourceRead::Payload(output),
        Some(401) => SourceRead::LoginNeeded,
        Some(403) => SourceRead::NoAccess,
        Some(429) => SourceRead::RateLimited {
            retry_after_sec: field(&text, "SOYEHT_RETRY:")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(300)
                .min(soyeht_protocol::limits::USAGE_MAX_BACKOFF_SEC as u32),
        },
        Some(500..=599) | Some(0) => SourceRead::NetworkError,
        Some(_) | None => SourceRead::Unavailable(None),
    }
}

fn field<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix(prefix).map(str::trim))
}

fn find_last(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

fn valid_token(token: &[u8]) -> bool {
    !token.is_empty()
        && token.len() <= 4096
        && token
            .iter()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
}

fn run_bounded(program: &PathBuf, args: &[&str], input: Option<&[u8]>) -> Option<Output> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = command.spawn().ok()?;
    if let Some(bytes) = input {
        let Some(mut stdin) = child.stdin.take() else {
            stop_child(&mut child);
            return None;
        };
        if stdin.write_all(bytes).is_err() {
            stop_child(&mut child);
            return None;
        }
    }
    let Some(stdout) = child.stdout.take() else {
        stop_child(&mut child);
        return None;
    };
    let reader = thread::spawn(move || read_bounded(stdout));
    let success = wait_child(&mut child, Instant::now() + SOURCE_TIMEOUT)?;
    let bytes = reader.join().ok()??;
    Some(Output { success, bytes })
}

fn stop_child(child: &mut Child) {
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

fn wait_child(child: &mut Child, deadline: Instant) -> Option<bool> {
    loop {
        if let Some(status) = child.try_wait().ok()? {
            return Some(status.success());
        }
        if Instant::now() >= deadline {
            stop_child(child);
            return None;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn app_server_read(program: &PathBuf) -> Result<Vec<u8>, UsageUnavailableReason> {
    let mut child = Command::new(program)
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                UsageUnavailableReason::ToolNotFound
            } else {
                UsageUnavailableReason::AppServerFailed
            }
        })?;
    let result = (|| {
        let mut stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let receiver = spawn_line_reader(stdout);
        let deadline = Instant::now() + SOURCE_TIMEOUT;
        write_json_line(
            &mut stdin,
            &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"soyeht","title":"Soyeht","version":env!("CARGO_PKG_VERSION")}}}),
        )?;
        response_with_id(&receiver, 1, deadline)?;
        write_json_line(
            &mut stdin,
            &serde_json::json!({"jsonrpc":"2.0","method":"initialized"}),
        )?;
        write_json_line(
            &mut stdin,
            &serde_json::json!({"jsonrpc":"2.0","id":2,"method":"account/rateLimits/read","params":{}}),
        )?;
        response_with_id(&receiver, 2, deadline)
    })();
    stop_child(&mut child);
    result.ok_or(UsageUnavailableReason::AppServerFailed)
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
        for _ in 0..64 {
            let mut bytes = Vec::new();
            let count = reader
                .by_ref()
                .take((limits::JSON_BYTES + 1) as u64)
                .read_until(b'\n', &mut bytes)
                .unwrap_or(0);
            if count == 0 {
                let _ = sender.send(Line::End);
                return;
            }
            if bytes.len() > limits::JSON_BYTES {
                let _ = sender.send(Line::Oversized);
                return;
            }
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
            }
            if sender.send(Line::Bytes(bytes)).is_err() {
                return;
            }
        }
        let _ = sender.send(Line::Oversized);
    });
    receiver
}

fn response_with_id(receiver: &Receiver<Line>, id: u64, deadline: Instant) -> Option<Vec<u8>> {
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        match receiver.recv_timeout(remaining).ok()? {
            Line::Bytes(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                if value["id"] == id {
                    return value.get("result").map(|_| bytes);
                }
            }
            Line::Oversized | Line::End => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    use uuid::Uuid;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("sn-usage-source-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn script(&self, name: &str, body: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).unwrap();
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn codex_reader_distinguishes_missing_tool_from_failed_app_server() {
        let fixture = Fixture::new();
        let missing = fixture.0.join("not-installed");
        let broken = fixture.script("broken", "exit 1");
        assert!(matches!(
            SystemSources::with_commands(Commands {
                claude_cache: missing.clone(),
                second_cli: missing.clone(),
                keychain: missing.clone(),
                http: missing.clone(),
            })
            .read(UsageProvider::Codex, UsageSource::CodexAppServer),
            SourceRead::Unavailable(Some(UsageUnavailableReason::ToolNotFound))
        ));
        assert!(matches!(
            SystemSources::with_commands(Commands {
                claude_cache: missing.clone(),
                second_cli: broken,
                keychain: missing.clone(),
                http: missing,
            })
            .read(UsageProvider::Codex, UsageSource::CodexAppServer),
            SourceRead::Unavailable(Some(UsageUnavailableReason::AppServerFailed))
        ));
    }

    #[test]
    fn passive_cache_and_explicit_process_sources_return_bounded_payloads() {
        let fixture = Fixture::new();
        let claude_cache = fixture.0.join("claude-cache.json");
        let second_log = fixture.0.join("second.input");
        let http_args = fixture.0.join("http.args");
        let http_input = fixture.0.join("http.input");
        fs::write(
            &claude_cache,
            br#"{"cachedUsageUtilization":{"fetchedAtMs":1000,"utilization":{}}}"#,
        )
        .unwrap();
        let second = fixture.script(
            "second",
            &format!(
                "read first; printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'; read initialized; read request; printf '%s\\n%s\\n%s\\n' \"$first\" \"$initialized\" \"$request\" > '{}'; printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"rateLimits\":{{}}}}}}'",
                second_log.display()
            ),
        );
        let keychain = fixture.script(
            "keychain",
            "case \"$*\" in *-w*) printf secret-token;; *) exit 0;; esac",
        );
        let http = fixture.script(
            "http",
            &format!(
                "printf '%s' \"$*\" > '{}'; cat > '{}'; printf '%s' '{{\"billingCycleEnd\":\"later\",\"planUsage\":{{\"totalPercentUsed\":25}}}}'; printf '\\nSOYEHT_STATUS:200\\nSOYEHT_RETRY:\\n'",
                http_args.display(),
                http_input.display()
            ),
        );
        let mut sources = SystemSources::with_commands(Commands {
            claude_cache,
            second_cli: second,
            keychain,
            http,
        });
        assert!(matches!(
            sources.read(UsageProvider::Claude, UsageSource::ClaudeDesktopCache),
            SourceRead::Payload(bytes) if bytes == br#"{"cachedUsageUtilization":{"fetchedAtMs":1000,"utilization":{}}}"#
        ));
        assert!(matches!(
            sources.read(UsageProvider::Codex, UsageSource::CodexAppServer),
            SourceRead::Payload(bytes) if serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"] == 2
        ));
        assert!(matches!(
            sources.read(UsageProvider::Cursor, UsageSource::CursorCurrentPeriod),
            SourceRead::Payload(bytes) if serde_json::from_slice::<serde_json::Value>(&bytes).is_ok()
        ));
        let calls = fs::read_to_string(second_log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(calls[0]["method"], "initialize");
        assert_eq!(calls[1]["method"], "initialized");
        assert_eq!(calls[2]["method"], "account/rateLimits/read");
        let args = fs::read_to_string(http_args).unwrap();
        assert!(args.contains(CURSOR_ENDPOINT));
        assert!(!args.contains("secret-token"));
        assert_eq!(
            fs::read(http_input).unwrap(),
            b"header = \"Authorization: Bearer secret-token\"\n"
        );
    }

    #[test]
    fn missing_and_denied_keychain_items_are_distinct_without_exposing_a_secret() {
        let fixture = Fixture::new();
        let no_item = fixture.script("no-item", "exit 44");
        let denied = fixture.script("denied", "case \"$*\" in *-w*) exit 36;; *) exit 0;; esac");
        let unused = fixture.script("unused", "exit 1");
        for (keychain, expected) in [
            (no_item, SourceRead::LoginNeeded),
            (denied, SourceRead::NoAccess),
        ] {
            let mut sources = SystemSources::with_commands(Commands {
                claude_cache: unused.clone(),
                second_cli: unused.clone(),
                keychain,
                http: unused.clone(),
            });
            assert!(
                sources.read(UsageProvider::Cursor, UsageSource::CursorCurrentPeriod) == expected
            );
        }
    }

    #[test]
    fn absent_claude_cache_is_unavailable_without_starting_a_process() {
        let fixture = Fixture::new();
        let spawn_marker = fixture.0.join("spawned");
        let unused = fixture.script(
            "must-not-run",
            &format!("touch '{}'; exit 1", spawn_marker.display()),
        );
        let mut sources = SystemSources::with_commands(Commands {
            claude_cache: fixture.0.join("absent.json"),
            second_cli: unused.clone(),
            keychain: unused.clone(),
            http: unused,
        });
        assert!(
            sources.read(UsageProvider::Claude, UsageSource::ClaudeDesktopCache)
                == SourceRead::Unavailable(None)
        );
        assert!(!spawn_marker.exists());
    }

    #[test]
    fn http_statuses_map_without_returning_response_bodies() {
        for (status, expected) in [
            (401, SourceRead::LoginNeeded),
            (403, SourceRead::NoAccess),
            (
                429,
                SourceRead::RateLimited {
                    retry_after_sec: 17,
                },
            ),
            (503, SourceRead::NetworkError),
        ] {
            let output = format!("private body\nSOYEHT_STATUS:{status}\nSOYEHT_RETRY:17\n");
            assert!(parse_http_output(output.into_bytes()) == expected);
        }
    }
}
