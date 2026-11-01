//! How a provider's usage reading is shown on the rail: the window the ring follows, its
//! colour band and the percentage under it. The thresholds, opacities and the weekly rule come
//! from `x-clientPolicy` (contract 10), the Mac's and the iPhone's source too; the cases in
//! `contracts/vectors/usage.json` check both implementations.

use crate::timestamp;
use soyeht_protocol::{
    UsageActivity, UsageProvider, UsageReading, UsageSource, UsageState, UsageUnavailableReason,
    UsageWindow, client_policy,
};

/// The colour band of a used fraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// Below the warning threshold.
    Success,
    /// From the warning threshold to the danger one, both included.
    Warning,
    /// Above the danger threshold.
    Danger,
}

impl Band {
    pub fn of(used: f64) -> Self {
        let used = used.clamp(0.0, 1.0);
        if used < client_policy::USAGE_BAND_WARNING_FROM {
            Self::Success
        } else if used <= client_policy::USAGE_BAND_DANGER_ABOVE {
            Self::Warning
        } else {
            Self::Danger
        }
    }
}

/// How far a reading can be trusted at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// A fresh reading: the band colour at full strength.
    Current,
    /// A stale reading: the percentage stays, the arc is muted.
    LastKnown,
    /// No reading: a dimmed ring and a dash.
    None,
}

impl Emphasis {
    pub fn arc_opacity(self) -> f32 {
        match self {
            Self::Current => 1.0,
            Self::LastKnown => client_policy::USAGE_RING_LAST_KNOWN_ARC_OPACITY as f32,
            Self::None => 0.0,
        }
    }

    pub fn ring_opacity(self) -> f32 {
        if self == Self::None {
            client_policy::USAGE_RING_UNAVAILABLE_OPACITY as f32
        } else {
            1.0
        }
    }
}

pub struct Presentation<'a>(pub &'a UsageReading);

impl Presentation<'_> {
    /// The window the ring follows: the weekly one for Claude, Codex and OpenCode, the only one
    /// for Cursor.
    pub fn window(&self) -> Option<&UsageWindow> {
        let reading = self.0;
        match reading.provider {
            UsageProvider::Cursor => reading.primary.as_ref(),
            UsageProvider::Claude | UsageProvider::Codex | UsageProvider::Opencode => {
                [&reading.primary, &reading.secondary]
                    .into_iter()
                    .flatten()
                    .find(|window| {
                        window
                            .label
                            .to_lowercase()
                            .contains(client_policy::USAGE_WINDOW_WEEKLY_KEYWORD)
                    })
                    .or(reading.secondary.as_ref())
                    .or(reading.primary.as_ref())
            }
        }
    }

    pub fn fraction(&self) -> Option<f64> {
        matches!(self.0.state, UsageState::Ok | UsageState::Stale)
            .then(|| self.window().map(|window| window.used.clamp(0.0, 1.0)))
            .flatten()
    }

    pub fn band(&self) -> Option<Band> {
        self.fraction().map(Band::of)
    }

    pub fn is_stale(&self) -> bool {
        self.0.state == UsageState::Stale
    }

    pub fn emphasis(&self) -> Emphasis {
        match (self.fraction(), self.is_stale()) {
            (None, _) => Emphasis::None,
            (Some(_), true) => Emphasis::LastKnown,
            (Some(_), false) => Emphasis::Current,
        }
    }

    /// Why there is no reading, in words; the sentence the usage detail shows.
    pub fn unavailable_message(&self) -> String {
        self.reason_message()
            .unwrap_or_else(|| "Usage is unavailable.".to_owned())
    }

    /// The one line the detail shows for a reading that holds an activity and no window, current
    /// or last known; `None` for any other reading.
    pub fn activity_notice(&self) -> Option<String> {
        let reading = self.0;
        let activity = reading.activity.as_ref().filter(|_| {
            reading.primary.is_none() && matches!(reading.state, UsageState::Ok | UsageState::Stale)
        })?;
        let mut text = format!(
            "{}: {}. No plan limit is known, so there is no percentage.",
            activity.label,
            activity_summary(activity)
        );
        if let Some(hint) = self.stale_hint() {
            text.push(' ');
            text.push_str(&hint);
        }
        Some(text)
    }

    /// Why a last-known reading was not replaced: the latest refresh failed for this reason.
    pub fn stale_hint(&self) -> Option<String> {
        self.is_stale()
            .then(|| self.reason_message())
            .flatten()
            .map(|reason| format!("Latest refresh: {reason}"))
    }

    fn reason_message(&self) -> Option<String> {
        let provider = self.0.provider.display_name();
        Some(match self.0.unavailable_reason.as_ref()? {
            UsageUnavailableReason::NotChecked => "Usage has not been checked yet.".to_owned(),
            UsageUnavailableReason::NoData => format!("{provider} has no usage data yet."),
            UsageUnavailableReason::ToolNotFound => format!("{provider} not found on this Mac."),
            UsageUnavailableReason::AppServerFailed => {
                format!("{provider} app server could not provide usage.")
            }
            UsageUnavailableReason::InvalidResponse => {
                "Usage reader returned invalid data.".to_owned()
            }
            UsageUnavailableReason::RateLimited => {
                format!("{provider} is limiting usage requests; retrying later.")
            }
            UsageUnavailableReason::NetworkError => format!("{provider} could not be reached."),
            UsageUnavailableReason::LoginNeeded => {
                format!("Sign in to {provider} in your terminal.")
            }
            UsageUnavailableReason::NoAccess => {
                format!("Access to the {provider} login was declined.")
            }
            UsageUnavailableReason::SourceFailed => format!("{provider} could not provide usage."),
            UsageUnavailableReason::PlanExpired => {
                format!("{provider}'s plan has expired or is not active.")
            }
        })
    }

    pub fn percentage(&self) -> String {
        self.fraction().map_or_else(
            || "\u{2014}".to_owned(),
            |f| format!("{}%", (f * 100.0).round()),
        )
    }
}

/// Where a reading came from, as the detail names it.
pub fn source_name(source: &UsageSource) -> &'static str {
    match source {
        UsageSource::ClaudeOauthEndpoint => "Claude Code login (OAuth)",
        UsageSource::ClaudeCliUsage => "Claude Code /usage",
        UsageSource::ClaudeWeb => "claude.ai browser session",
        UsageSource::ClaudeDesktopCache => "Claude Code cache",
        UsageSource::CodexAccessToken => "Codex access token",
        UsageSource::CodexAccountEndpoint => "Codex login (ChatGPT)",
        UsageSource::CodexAppServer => "Codex app server",
        UsageSource::CursorAppLogin => "Cursor app login",
        UsageSource::CursorBrowserCookies => "cursor.com browser session",
        UsageSource::CursorCurrentPeriod => "Cursor current period",
        UsageSource::OpencodeGoAllowance => "OpenCode Go allowance",
        UsageSource::OpencodeZaiPlan => "GLM Coding Plan quota",
        UsageSource::OpencodeLocalHistory => "OpenCode history on this device",
    }
}

/// Tokens as an exact count under 1,000, then one decimal with K, M or B (a trailing ".0"
/// dropped; a value that rounds to 1,000 moves up a unit).
fn tokens_text(count: i64) -> String {
    if count < 1_000 {
        return count.max(0).to_string();
    }
    let mut text = String::new();
    for (scale, suffix) in [(1e3, "K"), (1e6, "M"), (1e9, "B")] {
        let value = (count as f64 / scale * 10.0).round() / 10.0;
        text = if value == value.round() {
            format!("{}{suffix}", value as i64)
        } else {
            format!("{value:.1}{suffix}")
        };
        if value < 1_000.0 {
            break;
        }
    }
    text
}

/// What a provider with no plan limit recorded: "1.2M tokens · $3.40". It is a count, so
/// it has no percentage and no band (contract 10 section 4).
pub fn activity_summary(activity: &UsageActivity) -> String {
    let mut text = format!(
        "{} {}",
        tokens_text(activity.tokens),
        if activity.tokens == 1 {
            "token"
        } else {
            "tokens"
        }
    );
    if let Some(cost) = activity.cost_usd {
        text += &format!(" \u{b7} ${cost:.2}");
    }
    text
}

/// A span of time as the usage detail says it: "10m", "1h 30m", "1d 1h". Under a minute is a
/// minute.
pub fn duration_text(seconds: u64) -> String {
    let minutes = (seconds / 60).max(1);
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let (hours, remainder) = (minutes / 60, minutes % 60);
    if hours < 24 {
        return if remainder == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {remainder}m")
        };
    }
    format!("{}d {}h", hours / 24, hours % 24)
}

/// When a window resets, as the detail says it, `now` being seconds since the Unix epoch.
pub fn reset_text(resets_at: Option<&str>, now: f64) -> String {
    let Some(at) = resets_at.and_then(timestamp::seconds_since_epoch) else {
        return "Reset unavailable".to_owned();
    };
    if at > now {
        format!("Resets in {}", duration_text((at - now) as u64))
    } else {
        "Reset due".to_owned()
    }
}

/// How old a reading is: "updated 5m ago" for a current one, "as of" the minute for a stale one.
pub fn age_text(reading: &UsageReading, now: f64) -> String {
    let Some(fetched) = timestamp::seconds_since_epoch(&reading.fetched_at) else {
        return "time unavailable".to_owned();
    };
    if reading.state == UsageState::Stale {
        let minutes_of_day = (fetched as i64).rem_euclid(86_400) / 60;
        return format!(
            "as of {:02}:{:02}",
            minutes_of_day / 60,
            minutes_of_day % 60
        );
    }
    let age = now - fetched;
    if age >= 60.0 {
        format!("updated {} ago", duration_text(age as u64))
    } else {
        "updated just now".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::cases;
    use serde_json::json;

    fn reading(provider: &str, state: &str, windows: &[(&str, f64)]) -> UsageReading {
        let window = |(label, used): &(&str, f64)| json!({"used": used, "resetsAt": "2026-10-10T00:00:00Z", "label": label});
        serde_json::from_value(json!({
            "provider": provider, "state": state, "source": "claude_cli_usage",
            "fetchedAt": "2026-10-09T10:00:00Z",
            "primary": windows.first().map(window), "secondary": windows.get(1).map(window)
        }))
        .unwrap()
    }

    fn band_name(band: Band) -> &'static str {
        match band {
            Band::Success => "success",
            Band::Warning => "warning",
            Band::Danger => "danger",
        }
    }

    fn vectors() -> serde_json::Value {
        crate::testing::vector("usage-presentation")
    }

    #[test]
    fn readings_follow_the_shared_vectors() {
        for case in vectors()["readings"].as_array().unwrap() {
            let reading: UsageReading = serde_json::from_value(case["reading"].clone()).unwrap();
            let shown = Presentation(&reading);
            let name = &case["name"];
            assert_eq!(
                shown.window().map(|window| window.label.as_str()),
                case["window"].as_str(),
                "{name}"
            );
            assert_eq!(shown.percentage(), case["percentage"], "{name}");
            let band = shown.band().map(|band| format!("{band:?}").to_lowercase());
            assert_eq!(band.as_deref(), case["band"].as_str(), "{name}");
            let emphasis = match shown.emphasis() {
                Emphasis::Current => "current",
                Emphasis::LastKnown => "lastKnown",
                Emphasis::None => "none",
            };
            assert_eq!(emphasis, case["emphasis"], "{name}");
            assert_eq!(
                shown.unavailable_message(),
                case["unavailableMessage"],
                "{name}"
            );
            assert_eq!(
                shown.stale_hint().as_deref(),
                case["staleHint"].as_str(),
                "{name}"
            );
        }
    }

    #[test]
    fn durations_and_sources_follow_the_shared_vectors() {
        let vectors = vectors();
        for pair in vectors["durations"].as_array().unwrap() {
            assert_eq!(duration_text(pair[0].as_u64().unwrap()), pair[1]);
        }
        for pair in vectors["sources"].as_array().unwrap() {
            let source: UsageSource = serde_json::from_value(pair[0].clone()).unwrap();
            assert_eq!(source_name(&source), pair[1]);
        }
    }

    #[test]
    fn activities_follow_the_shared_vectors() {
        let vectors = vectors();
        for case in vectors["activities"]["cases"].as_array().unwrap() {
            let activity: UsageActivity = serde_json::from_value(case["activity"].clone()).unwrap();
            let name = &case["name"];
            assert_eq!(activity_summary(&activity), case["summary"], "{name}");
            assert_eq!(
                format!("{}: {}", activity.label, activity_summary(&activity)),
                case["line"],
                "{name}"
            );
        }
        for case in vectors["activities"]["notices"]["cases"]
            .as_array()
            .unwrap()
        {
            let reading: UsageReading = serde_json::from_value(case["reading"].clone()).unwrap();
            assert_eq!(
                Presentation(&reading).activity_notice().as_deref(),
                case["notice"].as_str(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn a_reset_is_said_the_way_the_shared_vector_says() {
        let vectors = vectors();
        let now =
            timestamp::seconds_since_epoch(vectors["resets"]["now"].as_str().unwrap()).unwrap();
        for case in vectors["resets"]["cases"].as_array().unwrap() {
            assert_eq!(
                reset_text(case["resetsAt"].as_str(), now),
                case["text"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn times_are_said_in_relation_to_now() {
        let at = |text| timestamp::seconds_since_epoch(text).unwrap();
        let now = at("2026-10-09T10:00:00Z");
        assert_eq!(
            reset_text(Some("2026-10-09T11:30:00Z"), now),
            "Resets in 1h 30m"
        );
        assert_eq!(reset_text(Some("2026-10-09T09:00:00Z"), now), "Reset due");
        assert_eq!(reset_text(None, now), "Reset unavailable");
        let current = reading("claude", "ok", &[("Weekly", 0.1)]);
        assert_eq!(age_text(&current, now + 300.0), "updated 5m ago");
        assert_eq!(age_text(&current, now + 5.0), "updated just now");
        let stale = reading("claude", "stale", &[("Weekly", 0.1)]);
        assert_eq!(age_text(&stale, now + 300.0), "as of 10:00");
    }

    #[test]
    fn the_bands_split_where_the_vector_says() {
        for case in cases("usage", "bands") {
            let used = case["used"].as_f64().unwrap();
            assert_eq!(band_name(Band::of(used)), case["band"], "{used}");
        }
    }

    #[test]
    fn the_opacities_are_the_vectors() {
        for case in cases("usage", "emphasis") {
            let emphasis = match case["emphasis"].as_str().unwrap() {
                "current" => Emphasis::Current,
                "last_known" => Emphasis::LastKnown,
                _ => Emphasis::None,
            };
            assert_eq!(
                emphasis.arc_opacity(),
                case["arcOpacity"].as_f64().unwrap() as f32
            );
            assert_eq!(
                emphasis.ring_opacity(),
                case["ringOpacity"].as_f64().unwrap() as f32
            );
        }
    }
}
