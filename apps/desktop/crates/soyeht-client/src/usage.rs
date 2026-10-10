//! How a provider's usage reading is shown on the rail: the window the ring follows, its
//! colour band and the percentage under it. The thresholds are the ones of contract 10, as on
//! the Mac and the iPhone.

use soyeht_protocol::{UsageProvider, UsageReading, UsageState, UsageWindow};

/// The colour band of a used fraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// Below 50 %.
    Success,
    /// From 50 % to 80 %, both included.
    Warning,
    /// Above 80 %.
    Danger,
}

impl Band {
    pub fn of(used: f64) -> Self {
        let used = used.clamp(0.0, 1.0);
        if used < 0.5 {
            Self::Success
        } else if used <= 0.8 {
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
            Self::LastKnown => 0.45,
            Self::None => 0.0,
        }
    }

    pub fn ring_opacity(self) -> f32 {
        if self == Self::None { 0.46 } else { 1.0 }
    }
}

pub struct Presentation<'a>(pub &'a UsageReading);

impl Presentation<'_> {
    /// The window the ring follows: the weekly one for Claude and Codex, the only one for
    /// Cursor.
    pub fn window(&self) -> Option<&UsageWindow> {
        let reading = self.0;
        match reading.provider {
            UsageProvider::Cursor => reading.primary.as_ref(),
            UsageProvider::Claude | UsageProvider::Codex => [&reading.primary, &reading.secondary]
                .into_iter()
                .flatten()
                .find(|window| window.label.to_lowercase().contains("week"))
                .or(reading.secondary.as_ref())
                .or(reading.primary.as_ref()),
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

    pub fn percentage(&self) -> String {
        self.fraction().map_or_else(
            || "\u{2014}".to_owned(),
            |f| format!("{}%", (f * 100.0).round()),
        )
    }
}

pub fn display_name(provider: &UsageProvider) -> &'static str {
    match provider {
        UsageProvider::Claude => "Claude",
        UsageProvider::Codex => "Codex",
        UsageProvider::Cursor => "Cursor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn the_bands_split_at_half_and_four_fifths() {
        assert_eq!(Band::of(0.49), Band::Success);
        assert_eq!(Band::of(0.5), Band::Warning);
        assert_eq!(Band::of(0.8), Band::Warning);
        assert_eq!(Band::of(0.81), Band::Danger);
    }

    #[test]
    fn claude_and_codex_follow_the_weekly_window() {
        let claude = reading("claude", "ok", &[("5-hour", 0.9), ("Weekly", 0.08)]);
        let shown = Presentation(&claude);
        assert_eq!(shown.percentage(), "8%");
        assert_eq!(shown.band(), Some(Band::Success));
        assert_eq!(shown.emphasis(), Emphasis::Current);
    }

    #[test]
    fn cursor_follows_its_only_window() {
        let cursor = reading("cursor", "ok", &[("Month", 0.55)]);
        assert_eq!(Presentation(&cursor).percentage(), "55%");
    }

    #[test]
    fn a_stale_reading_keeps_its_percentage_and_mutes_the_arc() {
        let stale = reading("codex", "stale", &[("Weekly", 0.07)]);
        let shown = Presentation(&stale);
        assert_eq!(shown.percentage(), "7%");
        assert_eq!(shown.emphasis(), Emphasis::LastKnown);
    }

    #[test]
    fn no_reading_shows_a_dash_and_a_dimmed_ring() {
        let lost = reading("claude", "login_needed", &[]);
        let shown = Presentation(&lost);
        assert_eq!(shown.percentage(), "\u{2014}");
        assert_eq!(shown.emphasis(), Emphasis::None);
        assert!(shown.emphasis().ring_opacity() < 1.0);
    }
}
