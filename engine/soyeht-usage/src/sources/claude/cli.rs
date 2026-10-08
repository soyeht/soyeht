//! A hidden Claude Code session asked for `/usage` (CodexBar `ClaudeCLISession`,
//! `ClaudeStatusProbe`, `ClaudeProbeSessionArtifactCleaner`).

use super::super::{Attempt, Failure, ReadContext, Source};
use super::screen::{self, ScreenOutcome};
use crate::host::{Host, TerminalDriver, TerminalRequest, TerminalRun, TerminalStep};
use soyeht_protocol::UsageSource;
use std::{path::Path, time::Duration};

pub(crate) const PROGRAM: &str = "claude";
/// CodexBar reuses a background CLI reading for 15 minutes instead of starting Claude again
/// (`ClaudeCLIUsageSpawnThrottle`).
const AUTOMATIC_REUSE_SEC: u64 = 15 * 60;
const TIMEOUT: Duration = Duration::from_secs(24);
/// Claude Code needs a moment to draw its prompt before it accepts a command.
const READY_AFTER: Duration = Duration::from_secs(2);
const ENTER_EVERY: Duration = Duration::from_millis(800);
const SETTLE: Duration = Duration::from_secs(2);
const PROBE_DIRECTORY: &str = "soyeht-usage-claude";
const LOCAL_SETTINGS: &str = r#"{"disableDeepLinkRegistration":"disable"}"#;

pub(crate) struct CliSource;

impl Source for CliSource {
    fn id(&self) -> UsageSource {
        UsageSource::ClaudeCliUsage
    }

    fn automatic_reuse_sec(&self) -> Option<u64> {
        Some(AUTOMATIC_REUSE_SEC)
    }

    fn read(&self, host: &dyn Host, context: ReadContext) -> Attempt {
        let workdir = host.temp_dir().join(PROBE_DIRECTORY);
        let session = uuid::Uuid::new_v4().to_string();
        let transcript = super::config_root(host).map(|root| {
            root.join("projects")
                .join(project_key(&workdir))
                .join(format!("{session}.jsonl"))
        });
        let request = TerminalRequest {
            program: PROGRAM.into(),
            args: vec![
                "--allowed-tools".into(),
                String::new(),
                "--strict-mcp-config".into(),
                "--settings".into(),
                r#"{"remoteControlAtStartup":false}"#.into(),
                "--session-id".into(),
                session,
            ],
            env: vec![
                ("DISABLE_AUTOUPDATER".into(), "1".into()),
                ("PWD".into(), workdir.display().to_string()),
            ],
            workdir,
            files: vec![(
                ".claude/settings.local.json".into(),
                LOCAL_SETTINGS.as_bytes().to_vec(),
            )],
            transcript,
            timeout: TIMEOUT,
        };
        let mut driver = UsageDriver::default();
        match host.run_terminal(&request, &mut driver) {
            TerminalRun::Finished(text) => match screen::parse(&text, context.now_sec) {
                ScreenOutcome::Reading(reading) => Ok(reading),
                ScreenOutcome::Failed(failure) => Err(failure),
            },
            TerminalRun::ToolNotFound => Err(Failure::ToolNotFound),
            TerminalRun::Incomplete => Err(Failure::Failed),
        }
    }
}

/// Claude Code's directory name for a project: every character that is not an ASCII letter or
/// digit becomes `-`; names longer than 200 keep 200 characters plus a hash suffix.
pub(crate) fn project_key(path: &Path) -> String {
    let text = path.display().to_string();
    let sanitized: String = text
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.len() <= 200 {
        return sanitized;
    }
    let mut hash: i32 = 0;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    format!(
        "{}-{}",
        &sanitized[..200],
        base36(i64::from(hash).unsigned_abs())
    )
}

fn base36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Types `/usage` once the prompt is ready, accepts the trust dialog of the dedicated probe
/// directory, confirms the command palette, and stops two seconds after the usage panel shows.
#[derive(Default)]
pub(crate) struct UsageDriver {
    ready_at: Option<Duration>,
    command_sent: bool,
    trust_answers: u8,
    palette_confirmed: bool,
    continue_confirmed: bool,
    last_enter: Duration,
    found_at: Option<Duration>,
}

const TRUST_ANSWERS: u8 = 4;

impl TerminalDriver for UsageDriver {
    fn step(&mut self, screen_text: &str, elapsed: Duration) -> TerminalStep {
        let normalized = screen::normalized(screen_text);
        if let Some(found_at) = self.found_at {
            return if elapsed >= found_at + SETTLE {
                TerminalStep::Done
            } else {
                TerminalStep::Wait
            };
        }
        if let Some(keys) = trust_keys(screen_text, &normalized) {
            self.ready_at = Some(elapsed + READY_AFTER);
            if self.trust_answers < TRUST_ANSWERS {
                self.trust_answers += 1;
                return TerminalStep::Send(keys);
            }
            return TerminalStep::Wait;
        }
        if !self.command_sent {
            if elapsed < *self.ready_at.get_or_insert(READY_AFTER) {
                return TerminalStep::Wait;
            }
            self.command_sent = true;
            self.last_enter = elapsed;
            return TerminalStep::Send(b"/usage\r".to_vec());
        }
        if screen::usage_ready(&normalized) {
            self.found_at = Some(elapsed);
            return TerminalStep::Wait;
        }
        if !self.palette_confirmed && normalized.contains("showplan") {
            self.palette_confirmed = true;
            return TerminalStep::Send(b"\r".to_vec());
        }
        if !self.continue_confirmed && normalized.contains("pressentertocontinue") {
            self.continue_confirmed = true;
            return TerminalStep::Send(b"\r".to_vec());
        }
        if elapsed >= self.last_enter + ENTER_EVERY {
            self.last_enter = elapsed;
            return TerminalStep::Send(b"\r".to_vec());
        }
        TerminalStep::Wait
    }
}

/// The keys that accept the workspace trust dialog. The probe directory is Soyeht's own empty
/// scratch directory, so it is the one place where accepting is safe (as in CodexBar).
fn trust_keys(screen_text: &str, normalized: &str) -> Option<Vec<u8>> {
    let lines: Vec<&str> = screen_text.lines().collect();
    let option = lines
        .iter()
        .position(|line| screen::normalized(line).contains("yes,itrustthisfolder"));
    let Some(option) = option else {
        if normalized.contains("doyoutrustthefilesinthisfolder?") {
            return Some(b"y\r".to_vec());
        }
        if normalized.contains("readytocodehere?") || normalized.contains("quicksafetycheck:") {
            return Some(b"\r".to_vec());
        }
        return None;
    };
    let blank = |row: usize| lines[row].trim().is_empty();
    let mut first = option;
    while first > 0 && !blank(first - 1) {
        first -= 1;
    }
    let mut last = option;
    while last + 1 < lines.len() && !blank(last + 1) {
        last += 1;
    }
    let marker = (first..=last).find(|row| lines[*row].contains('❯'))?;
    Some(match marker.cmp(&option) {
        std::cmp::Ordering::Equal => b"\r".to_vec(),
        std::cmp::Ordering::Less => b"\x1b[B".to_vec(),
        std::cmp::Ordering::Greater => b"\x1b[A".to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_key_matches_claude_codes_directory_names() {
        assert_eq!(
            project_key(Path::new("/private/var/folders/x_y/T/soyeht-usage-claude")),
            "-private-var-folders-x-y-T-soyeht-usage-claude"
        );
        let long = format!("/{}", "a".repeat(240));
        let key = project_key(Path::new(&long));
        assert!(key.starts_with(&format!("-{}", "a".repeat(199))));
        assert!(key.len() > 201);
    }

    #[test]
    fn driver_accepts_trust_types_usage_and_settles() {
        let mut driver = UsageDriver::default();
        let trust = "Do you trust the files in this folder?\n\n ❯ 1. Yes, I trust this folder\n   2. No, exit\n";
        assert_eq!(
            driver.step(trust, Duration::from_millis(500)),
            TerminalStep::Send(b"\r".to_vec())
        );
        assert_eq!(
            driver.step("> ", Duration::from_secs(1)),
            TerminalStep::Wait
        );
        assert_eq!(
            driver.step("> ", Duration::from_secs(3)),
            TerminalStep::Send(b"/usage\r".to_vec())
        );
        let panel = "Current session\n███ 15% used\nResets 3pm\n";
        assert_eq!(
            driver.step(panel, Duration::from_secs(4)),
            TerminalStep::Wait
        );
        assert_eq!(
            driver.step(panel, Duration::from_secs(5)),
            TerminalStep::Wait
        );
        assert_eq!(
            driver.step(panel, Duration::from_secs(6)),
            TerminalStep::Done
        );
    }

    #[test]
    fn trust_marker_above_the_yes_option_moves_down_first() {
        let screen = " ❯ 1. No, exit\n   2. Yes, I trust this folder\n";
        assert_eq!(
            trust_keys(screen, &screen::normalized(screen)),
            Some(b"\x1b[B".to_vec())
        );
    }
}
