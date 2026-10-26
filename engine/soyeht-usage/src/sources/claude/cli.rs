//! A hidden Claude Code session asked for `/usage` (CodexBar `ClaudeCLISession`,
//! `ClaudeStatusProbe`, `ClaudeProbeSessionArtifactCleaner`).
//!
//! The session is never started without a stored login, and it stops at the first screen that
//! waits for a person. Both are needed: a signed-out Claude Code opens its sign-in page in the
//! browser as soon as a key chooses a login method, and this probe runs every few minutes.

use super::super::{Attempt, Failure, ReadContext, Source, default_cooldown_sec};
use super::{
    oauth,
    screen::{self, ScreenOutcome},
};
use crate::host::{Host, TerminalDriver, TerminalRequest, TerminalRun, TerminalStep};
use soyeht_protocol::UsageSource;
use std::{path::Path, time::Duration};

pub(crate) const PROGRAM: &str = "claude";
/// CodexBar reuses a background CLI reading for 15 minutes instead of starting Claude again
/// (`ClaudeCLIUsageSpawnThrottle`).
const AUTOMATIC_REUSE_SEC: u64 = 15 * 60;
/// How long automatic refreshes leave the session alone after it answered "sign in needed" or
/// failed (a screen it does not know stays unknown): the longest usage backoff. A signed-out
/// account stays signed out until the person acts, and the stored login is still read by the
/// other sources on every refresh, so signing in is noticed without the session.
const HELD_BACK_SEC: u64 = soyeht_protocol::limits::USAGE_MAX_BACKOFF_SEC as u64;
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

    fn automatic_cooldown_sec(&self, failure: &Failure) -> Option<u64> {
        match failure {
            Failure::LoginNeeded | Failure::Failed => Some(HELD_BACK_SEC),
            other => default_cooldown_sec(other),
        }
    }

    fn read(&self, host: &dyn Host, context: ReadContext) -> Attempt {
        oauth::stored_login(host, &context)?;
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
///
/// It presses a key only on a screen it knows: the trust dialog, the empty input line, the
/// typed command, the palette and its "press enter" notice. On any other screen, blank or a menu
/// it has never seen, it waits, and the deadline ends the read. A guess here can choose a login
/// method, and the choice opens the browser.
#[derive(Default)]
pub(crate) struct UsageDriver {
    ready_at: Option<Duration>,
    command_typed: bool,
    trust_answers: u8,
    palette_confirmed: bool,
    continue_confirmed: bool,
    renderer_dismissed: bool,
    enters: u8,
    last_enter: Duration,
    found_at: Option<Duration>,
}

const TRUST_ANSWERS: u8 = 4;
/// Enter presses allowed while the typed command is still on the input line.
const COMMAND_ENTERS: u8 = 5;
const COMMAND: &str = "/usage";

impl TerminalDriver for UsageDriver {
    fn step(&mut self, screen_text: &str, elapsed: Duration) -> TerminalStep {
        let normalized = screen::normalized(screen_text);
        if screen::waits_for_a_person(&normalized) && !normalized.contains("currentsession") {
            return TerminalStep::Done;
        }
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
        // A notice that offers a new renderer: Escape cancels it and chooses nothing.
        if !self.renderer_dismissed && normalized.contains("trythenewfullscreenrenderer?") {
            self.renderer_dismissed = true;
            self.ready_at = Some(elapsed + READY_AFTER);
            return TerminalStep::Send(b"\x1b".to_vec());
        }
        let input = input_line(screen_text);
        if !self.command_typed {
            let settled = elapsed >= *self.ready_at.get_or_insert(READY_AFTER);
            if settled && input.is_some() {
                self.command_typed = true;
                self.last_enter = elapsed;
                return TerminalStep::Send(COMMAND.as_bytes().to_vec());
            }
            return TerminalStep::Wait;
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
        let typed = input.is_some_and(|input| input.starts_with(COMMAND));
        if typed && self.enters < COMMAND_ENTERS && elapsed >= self.last_enter + ENTER_EVERY {
            self.enters += 1;
            self.last_enter = elapsed;
            return TerminalStep::Send(b"\r".to_vec());
        }
        TerminalStep::Wait
    }
}

/// The text after the prompt mark of the input line, or `None` when the screen has none.
///
/// The input line is the one that starts with the prompt mark (`❯` or `>`, inside a box or not)
/// right under a rule or the top of a box, as Claude Code draws it. A numbered menu is not one,
/// although its selected row starts with the same mark, and neither is a stray `>` line.
fn input_line(screen_text: &str) -> Option<String> {
    let lines: Vec<&str> = screen_text.lines().map(str::trim).collect();
    let numbered = |line: &str| {
        let line = line.trim_matches('│').trim().trim_start_matches(['❯', '>']);
        let line = line.trim_start();
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        digits > 0 && line[digits..].starts_with(". ")
    };
    if lines.iter().any(|line| numbered(line)) {
        return None;
    }
    let under_a_rule = |index: usize| {
        index > 0 && lines[index - 1].starts_with(['─', '╭']) && lines[index - 1].len() > 8
    };
    lines.iter().enumerate().find_map(|(index, line)| {
        let rest = line.trim_matches('│').trim().strip_prefix(['❯', '>'])?;
        under_a_rule(index).then(|| rest.trim().to_owned())
    })
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

    /// A real prompt of Claude Code, recorded in a pseudo terminal.
    const REPL: &str = include_str!("../../../tests/fixtures/claude-cli-repl.txt");

    fn sent(driver: &mut UsageDriver, screens: &[&str], seconds: u64) -> Vec<Vec<u8>> {
        let mut keys = Vec::new();
        for tick in 0..seconds * 2 {
            let screen = screens[usize::try_from(tick).unwrap().min(screens.len() - 1)];
            if let TerminalStep::Send(bytes) =
                driver.step(screen, Duration::from_millis(tick * 500))
            {
                keys.push(bytes);
            }
        }
        keys
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
            driver.step(REPL, Duration::from_secs(1)),
            TerminalStep::Wait
        );
        assert_eq!(
            driver.step(REPL, Duration::from_secs(3)),
            TerminalStep::Send(b"/usage".to_vec())
        );
        let typed = REPL.replace("Try \"create a util logging.py that...\"", "/usage");
        assert!(typed.contains("/usage"), "{typed}");
        assert_eq!(
            driver.step(&typed, Duration::from_secs(4)),
            TerminalStep::Send(b"\r".to_vec())
        );
        let panel = "Current session\n███ 15% used\nResets 3pm\n";
        assert_eq!(
            driver.step(panel, Duration::from_secs(5)),
            TerminalStep::Wait
        );
        assert_eq!(
            driver.step(panel, Duration::from_secs(6)),
            TerminalStep::Wait
        );
        assert_eq!(
            driver.step(panel, Duration::from_secs(7)),
            TerminalStep::Done
        );
    }

    #[test]
    fn the_new_renderer_notice_is_cancelled_with_escape_only() {
        let notice = "Try the new fullscreen renderer?\n· Flicker-free output\n❯ 1. Yes, try it\n  2. Not now\n\nEnter to confirm · Esc to cancel\n";
        let mut driver = UsageDriver::default();
        let keys = sent(&mut driver, &[notice], 10);
        assert_eq!(keys, vec![b"\x1b".to_vec()], "{keys:?}");
    }

    #[test]
    fn the_command_is_typed_alone_and_enter_waits_for_it_to_show() {
        let mut driver = UsageDriver::default();
        assert_eq!(
            sent(&mut driver, &[REPL], 4),
            vec![b"/usage".to_vec()],
            "the command went with its Enter, or twice"
        );
    }

    #[test]
    fn a_blank_or_unknown_screen_gets_no_key() {
        let menus = [
            "",
            "Starting...\n",
            "Sign in to continue\n\n ❯ 1. Use your Claude subscription\n   2. Use an API key\n",
            "Welcome\n> Press any key to open your browser\n",
        ];
        for menu in menus {
            let mut driver = UsageDriver::default();
            assert!(sent(&mut driver, &[menu], 30).is_empty(), "{menu:?}");
        }
        // Blank at first, a menu with new wording later: nothing was queued in between.
        let mut driver = UsageDriver::default();
        assert!(sent(&mut driver, &["", "", "", "", "", menus[2]], 30).is_empty());
    }

    #[test]
    fn a_menu_that_replaces_the_prompt_gets_no_more_keys() {
        let mut driver = UsageDriver::default();
        assert_eq!(sent(&mut driver, &[REPL], 4), vec![b"/usage".to_vec()]);
        let menu = "Session expired\n\n ❯ 1. Log in again\n   2. Quit\n";
        let later: Vec<Vec<u8>> = (8..40)
            .filter_map(
                |tick| match driver.step(menu, Duration::from_millis(tick * 500)) {
                    TerminalStep::Send(bytes) => Some(bytes),
                    _ => None,
                },
            )
            .collect();
        assert!(later.is_empty(), "{later:?}");
    }

    #[test]
    fn a_login_menu_ends_the_read_without_a_key() {
        let mut driver = UsageDriver::default();
        let menu = "Select login method:\n ❯ 1. Claude account with subscription\n";
        assert_eq!(
            driver.step(menu, Duration::from_secs(5)),
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
