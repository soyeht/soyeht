//! The usage ring of an agent pane's header: the meter of the provider that reads the pane's harness, as
//! the Mac and the iPhone draw it. Which provider reads which harness is the contract's rule
//! (`UsageProvider::for_harness`, `contracts/vectors/usage.json`); the window the ring follows, its band, its
//! opacities and its percentage are `soyeht_client::usage::Presentation`'s, which the thresholds of
//! `x-clientPolicy` drive.

use soyeht_client::{
    State,
    usage::{Band, Presentation},
};
use soyeht_protocol::UsageProvider;

/// The colour band of the arc, which the view resolves to a palette role.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaugeBand {
    Success,
    Warning,
    Danger,
}

/// What the ring draws.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct UsageGauge {
    /// The provider's name, for the ring's description.
    pub provider: String,
    /// The used fraction of the window the ring follows, when the reading has one.
    pub fraction: Option<f64>,
    pub band: Option<GaugeBand>,
    pub arc_opacity: f32,
    pub ring_opacity: f32,
    /// "42%", or a dash when there is no reading to say.
    pub percentage: String,
}

pub(crate) fn gauge(state: &State, pane_id: &str) -> Option<UsageGauge> {
    let harness = &state.pane(pane_id)?.agent.as_ref()?.harness;
    let provider = UsageProvider::for_harness(harness)?;
    let reading = state
        .usage()
        .iter()
        .find(|reading| reading.provider == provider)?;
    let presentation = Presentation(reading);
    let emphasis = presentation.emphasis();
    Some(UsageGauge {
        provider: provider.display_name().to_owned(),
        fraction: presentation.fraction(),
        band: presentation.band().map(|band| match band {
            Band::Success => GaugeBand::Success,
            Band::Warning => GaugeBand::Warning,
            Band::Danger => GaugeBand::Danger,
        }),
        arc_opacity: emphasis.arc_opacity(),
        ring_opacity: emphasis.ring_opacity(),
        percentage: presentation.percentage(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use soyeht_client::{Event, Snapshot};

    fn reading(provider: &str, state: &str, used: f64) -> Value {
        json!({
            "provider": provider, "state": state, "source": "claude_cli_usage",
            "fetchedAt": "2026-11-04T12:00:00Z",
            "primary": {"label": "Weekly", "used": used, "resetsAt": "2026-11-10T00:00:00Z"},
        })
    }

    fn state(harness: &str, readings: Vec<Value>) -> State {
        let mut state = State::default();
        let pane = json!({
            "paneId": "a", "workspaceId": "w", "name": "Agent", "cwd": "/work", "mode": "agent",
            "state": "idle", "source": "none", "confidence": 0.0, "phrase": "", "generation": 1,
            "cols": 80, "rows": 24, "record": false,
            "agent": {"harness": harness, "version": "1", "fingerprint": "f", "options": [],
                "authMethods": [], "queue": [], "draft": false, "role": "worker"},
        });
        let workspace = json!({"workspaceId": "w", "name": "w", "path": "/work", "layout": {}});
        state.apply(Event::Connected(Snapshot {
            workspaces: vec![serde_json::from_value(workspace).unwrap()],
            panes: vec![serde_json::from_value(pane).unwrap()],
            ..Snapshot::default()
        }));
        let readings = readings
            .into_iter()
            .map(|value| serde_json::from_value(value).unwrap())
            .collect();
        state.apply(Event::Usage(readings));
        state
    }

    #[test]
    fn the_ring_is_the_meter_of_the_provider_that_reads_the_harness() {
        let state = state(
            "registry.codex-acp",
            vec![reading("claude", "ok", 0.1), reading("codex", "ok", 0.6)],
        );
        let gauge = gauge(&state, "a").unwrap();
        assert_eq!(gauge.provider, "Codex");
        assert_eq!(gauge.fraction, Some(0.6));
        assert_eq!(gauge.band, Some(GaugeBand::Warning));
        assert_eq!(gauge.percentage, "60%");
        assert_eq!((gauge.arc_opacity, gauge.ring_opacity), (1.0, 1.0));
    }

    #[test]
    fn the_band_follows_the_thresholds_of_the_contract() {
        for (used, band) in [
            (0.1, GaugeBand::Success),
            (0.5, GaugeBand::Warning),
            (0.8, GaugeBand::Warning),
            (0.95, GaugeBand::Danger),
        ] {
            let state = state("registry.claude-acp", vec![reading("claude", "ok", used)]);
            let shown = gauge(&state, "a").unwrap();
            assert_eq!(shown.band, Some(band), "{used}");
        }
    }

    #[test]
    fn a_stale_reading_keeps_its_percentage_and_mutes_the_arc() {
        let state = state("registry.claude-acp", vec![reading("claude", "stale", 0.3)]);
        let shown = gauge(&state, "a").unwrap();
        assert_eq!(shown.percentage, "30%");
        assert!(shown.arc_opacity < 1.0 && shown.arc_opacity > 0.0);
    }

    #[test]
    fn a_reading_with_no_window_is_a_dimmed_ring_and_a_dash() {
        let unavailable = json!({
            "provider": "claude", "state": "unavailable", "unavailableReason": "login_needed",
            "source": "claude_cli_usage", "fetchedAt": "2026-11-04T12:00:00Z",
        });
        let state = state("registry.claude-acp", vec![unavailable]);
        let shown = gauge(&state, "a").unwrap();
        assert_eq!((shown.fraction, shown.band), (None, None));
        assert_eq!(shown.percentage, "\u{2014}");
        assert!(shown.ring_opacity < 1.0);
    }

    #[test]
    fn a_harness_no_meter_reads_and_a_reading_not_yet_here_show_no_ring() {
        let state_of = |harness: &str, readings| state(harness, readings);
        assert!(
            gauge(
                &state_of("local.my-tool", vec![reading("claude", "ok", 0.1)]),
                "a"
            )
            .is_none()
        );
        assert!(gauge(&state_of("registry.claude-acp", Vec::new()), "a").is_none());
        assert!(gauge(&State::default(), "missing").is_none());
    }
}
