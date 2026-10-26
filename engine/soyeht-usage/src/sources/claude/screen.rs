//! Parses the rendered `/usage` panel of Claude Code (CodexBar `ClaudeStatusProbe.parse`).

use super::super::{Failure, ParsedReading, percent_window};
use super::{SESSION_LABEL, WEEKLY_LABEL};
use chrono::{
    DateTime, Datelike, Duration as Days, Local, NaiveDate, NaiveDateTime, NaiveTime,
    SecondsFormat, TimeZone, Utc,
};

pub(crate) enum ScreenOutcome {
    Reading(ParsedReading),
    Failed(Failure),
}

const PERCENT_LOOKAHEAD: usize = 12;
const RESET_LOOKAHEAD: usize = 14;
const INSIGHTS_MARKER: &str = "what'scontributingtoyourlimitsusage?";

/// Lowercase without whitespace, the form every marker is matched in.
pub(crate) fn normalized(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// What a signed-out Claude Code prints instead of the panel.
const SIGNED_OUT: [&str; 4] = ["/login", "claudelogin", "notauthenticated", "invalidapikey"];
/// The prompts that wait for a person: the first-run setup and the choice of a login method.
const WAITS_FOR_A_PERSON: [&str; 4] = [
    "choosethetextstyle",
    "selectloginmethod",
    "claudeaccountwithsubscription",
    "browserdidn'topen",
];

/// The screen asks the person to set the program up or to sign in. A background probe stops
/// here without pressing a key: the next key would choose a login method and open the browser.
pub(crate) fn waits_for_a_person(normalized_screen: &str) -> bool {
    WAITS_FOR_A_PERSON
        .iter()
        .any(|marker| normalized_screen.contains(marker))
}

/// The panel has a session line with a percentage, or the screen shows an error or a signed-out
/// session, so the driver can stop.
pub(crate) fn usage_ready(normalized_screen: &str) -> bool {
    if normalized_screen.contains("failedtoloadusagedata") {
        return true;
    }
    match normalized_screen.find("currentsession") {
        Some(at) => normalized_screen[at..].contains('%'),
        None => signed_out(normalized_screen),
    }
}

fn signed_out(normalized_screen: &str) -> bool {
    waits_for_a_person(normalized_screen)
        || SIGNED_OUT
            .iter()
            .any(|marker| normalized_screen.contains(marker))
}

pub(crate) fn parse(text: &str, now_sec: u64) -> ScreenOutcome {
    let lines = panel_lines(text);
    if let Some(failure) = failure(&lines) {
        return ScreenOutcome::Failed(failure);
    }
    let session = labelled_window(&lines, "currentsession", SESSION_LABEL, now_sec);
    let weekly = labelled_window(&lines, "currentweek(allmodels)", WEEKLY_LABEL, now_sec);
    // CodexBar requires the session; an account without one shows the week alone.
    let reading = match (session, weekly) {
        (Some(primary), secondary) => primary.and_then(|primary| {
            Ok(ParsedReading {
                primary,
                secondary: secondary.transpose()?,
            })
        }),
        (None, Some(primary)) => primary.map(|primary| ParsedReading {
            primary,
            secondary: None,
        }),
        (None, None) => Err(Failure::InvalidResponse),
    };
    match reading {
        Ok(reading) => ScreenOutcome::Reading(reading),
        Err(failure) => ScreenOutcome::Failed(failure),
    }
}

/// The lines of the last Settings panel, cut before the "What's contributing" insights, whose
/// tool names and percentages are not quota fields.
fn panel_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if let Some(cut) = lines
        .iter()
        .position(|line| normalized(line).contains(INSIGHTS_MARKER))
    {
        lines.truncate(cut);
    }
    if let Some(start) = lines
        .iter()
        .rposition(|line| normalized(line).contains("settings:"))
    {
        lines.drain(..start);
    }
    lines
}

fn failure(lines: &[String]) -> Option<Failure> {
    let all = normalized(&lines.join("\n"));
    if all.contains("doyoutrustthefilesinthisfolder?") && !all.contains("currentsession") {
        return Some(Failure::Failed);
    }
    let failure_line = lines
        .iter()
        .map(|line| normalized(line))
        .rfind(|line| line.contains("failedtoloadusagedata"));
    if let Some(line) = failure_line {
        if line.contains("rate_limit") || line.contains("ratelimited") {
            return Some(Failure::RateLimited {
                retry_after_sec: None,
            });
        }
        if line.contains("token_expired")
            || line.contains("tokenhasexpired")
            || line.contains("authentication_error")
        {
            return Some(Failure::LoginNeeded);
        }
        return Some(Failure::Failed);
    }
    if !all.contains("currentsession") && signed_out(&all) {
        return Some(Failure::LoginNeeded);
    }
    None
}

fn labelled_window(
    lines: &[String],
    label: &str,
    title: &str,
    now_sec: u64,
) -> Option<Result<crate::Window, Failure>> {
    let row = lines
        .iter()
        .position(|line| normalized(line).contains(label))?;
    let percent = lines
        .iter()
        .skip(row)
        .take(PERCENT_LOOKAHEAD)
        .find_map(|line| used_percent(line))?;
    let reset = lines
        .iter()
        .skip(row + 1)
        .take(RESET_LOOKAHEAD)
        .find_map(|line| reset_text(line))
        .and_then(|text| reset_time(&text, now_sec));
    Some(percent_window(percent, reset, title))
}

/// The used percentage on a line: "15% used" is used, "85% left" is converted.
fn used_percent(line: &str) -> Option<f64> {
    let percent_at = line.find('%')?;
    let digits: String = line[..percent_at]
        .trim_end()
        .chars()
        .rev()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let value: f64 = digits.parse().ok()?;
    if !(0.0..=100.0).contains(&value) {
        return None;
    }
    let lower = line.to_lowercase();
    if ["left", "remaining", "available"]
        .iter()
        .any(|word| lower.contains(word))
    {
        Some(100.0 - value)
    } else {
        Some(value)
    }
}

fn reset_text(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let at = lower.find("reset")?;
    let rest = &line[at..];
    let rest = rest
        .trim_start_matches(|character: char| character.is_alphabetic())
        .trim();
    Some(rest.to_owned()).filter(|text| !text.is_empty())
}

/// "3pm", "3:30pm", "15:00", "Oct 7, 2:59pm" or "Oct 7, 2026, 2:59pm", with an optional
/// "(Area/City)" suffix, in the local time zone; the next such moment after `now`.
pub(crate) fn reset_time(text: &str, now_sec: u64) -> Option<String> {
    let text = text.split('(').next().unwrap_or(text).trim();
    // "Oct 7 at 12:59pm" and "Oct 7, 12:59pm" are the same moment.
    let text = text.trim_start_matches("at ").replace(" at ", ", ");
    let text = text.trim();
    let now = Local
        .timestamp_opt(i64::try_from(now_sec).ok()?, 0)
        .single()?;
    let (date_part, time_part) = match text.rfind([',', ' ']) {
        Some(split) if text[..split].chars().any(char::is_alphabetic) => (
            Some(text[..split].trim().trim_end_matches(',')),
            text[split + 1..].trim(),
        ),
        _ => (None, text),
    };
    let time = clock(time_part)?;
    let resolved = match date_part {
        None => {
            let today = now.date_naive().and_time(time);
            let candidate = local(today)?;
            if candidate <= now {
                local(today + Days::days(1))?
            } else {
                candidate
            }
        }
        Some(date_text) => {
            let (month_day, year) = match date_text.rsplit_once(',') {
                Some((head, tail)) if tail.trim().len() == 4 => {
                    (head.trim(), tail.trim().parse::<i32>().ok())
                }
                _ => (date_text, None),
            };
            let year = year.unwrap_or_else(|| now.year());
            let date =
                NaiveDate::parse_from_str(&format!("{month_day} {year}"), "%b %d %Y").ok()?;
            let candidate = local(date.and_time(time))?;
            if candidate + Days::days(1) < now {
                local(date.with_year(year + 1)?.and_time(time))?
            } else {
                candidate
            }
        }
    };
    Some(
        resolved
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true),
    )
}

fn local(naive: NaiveDateTime) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&naive).earliest()
}

fn clock(text: &str) -> Option<NaiveTime> {
    let compact: String = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    let (body, offset) = if let Some(body) = compact.strip_suffix("am") {
        (body, Some(0))
    } else if let Some(body) = compact.strip_suffix("pm") {
        (body, Some(12))
    } else {
        (compact.as_str(), None)
    };
    let (hour, minute) = match body.split_once(':') {
        Some((hour, minute)) => (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?),
        None => (body.parse::<u32>().ok()?, 0),
    };
    let hour = match offset {
        Some(offset) if (1..=12).contains(&hour) => hour % 12 + offset,
        Some(_) => return None,
        None => hour,
    };
    NaiveTime::from_hms_opt(hour, minute, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: &str = "\
 Status   Config   Usage
 Settings:  Status   Config   Usage  (tab to cycle)

 Current session
 ███████▌                                          15% used
 Resets 3pm (America/Sao_Paulo)

 Current week (all models)
 ██████████████▌                                   29% used
 Resets Oct 7, 2:59pm (America/Sao_Paulo)

 What's contributing to your limits usage?
 Bash 90%
";

    #[test]
    fn panel_reads_session_and_weekly_and_ignores_insights() {
        let ScreenOutcome::Reading(reading) = parse(PANEL, 1_790_000_000) else {
            panic!("panel did not parse");
        };
        assert_eq!(reading.primary.used, 0.15);
        assert_eq!(reading.primary.label, SESSION_LABEL);
        assert!(!reading.primary.resets_at.is_empty());
        let weekly = reading.secondary.unwrap();
        assert_eq!(weekly.used, 0.29);
        assert_eq!(weekly.label, WEEKLY_LABEL);
    }

    #[test]
    fn left_percentages_convert_to_used() {
        assert_eq!(used_percent("  ███ 85% left"), Some(15.0));
        assert_eq!(used_percent("no number here"), None);
    }

    #[test]
    fn reset_times_resolve_forward_in_local_time() {
        let now = 1_790_000_000;
        let reset = reset_time("3pm (Area/City)", now).unwrap();
        let parsed = DateTime::parse_from_rfc3339(&reset).unwrap().timestamp();
        assert!(parsed > now as i64 && parsed <= now as i64 + 86_400);
        let dated = reset_time("Oct 7, 2:59pm", now).unwrap();
        assert_eq!(
            reset_time("Oct 7 at 2:59pm (Area/City)", now).unwrap(),
            dated
        );
        assert!(DateTime::parse_from_rfc3339(&dated).unwrap().timestamp() > now as i64 - 86_400);
        assert!(reset_time("soon", now).is_none());
    }

    #[test]
    fn errors_on_screen_map_to_failures() {
        let limited = "Failed to load usage data: {\"error\":{\"type\":\"rate_limit_error\"}}";
        assert!(matches!(
            parse(limited, 0),
            ScreenOutcome::Failed(Failure::RateLimited { .. })
        ));
        let expired = "Failed to load usage data: token_expired";
        assert!(matches!(
            parse(expired, 0),
            ScreenOutcome::Failed(Failure::LoginNeeded)
        ));
        assert!(matches!(
            parse("Invalid API key · Please run /login", 0),
            ScreenOutcome::Failed(Failure::LoginNeeded)
        ));
    }

    #[test]
    fn readiness_needs_a_session_percentage() {
        assert!(!usage_ready(&normalized("Current session\nLoading usage…")));
        assert!(usage_ready(&normalized("Current session\n██ 3% used")));
    }
}
