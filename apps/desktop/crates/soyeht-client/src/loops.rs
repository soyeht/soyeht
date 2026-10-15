//! A pane's loop: a prompt delivered on a schedule when the pane is idle. This is what the pane
//! header's loop button opens, as on the Mac (`LoopPresentation`, `PaneAutomationPanel`); both
//! clients pass the cases of `contracts/vectors/loop-panel.json`.

use crate::state::{Effect, State};
use serde_json::Value;
use soyeht_protocol::{EventName, LoopConfig, LoopStopWhen, LoopStoppedEvent, LoopTickEvent};

pub const TITLE: &str = "Loop this pane";
pub const PROMPT_HEADING: &str = "PROMPT";
pub const WHEN_HEADING: &str = "WHEN";
pub const EVERY_INTERVAL: &str = "Every interval";
pub const WHEN_IDLE: &str = "When idle";
pub const WORKER_ROLE: &str = "Worker";
pub const LEAD_ROLE: &str = "Lead";
pub const CANCEL: &str = "Cancel";
pub const APPLY: &str = "Apply";
pub const RULES: [&str; 3] = [
    "Never sends over your draft.",
    "Busy ticks are skipped and recorded.",
    "Survives an engine restart.",
];

pub const DEFAULT_PROMPT: &str = "Pick the next task and complete it.";
pub const DEFAULT_INTERVAL: u32 = 60;
pub const DEFAULT_MAX_ROUNDS: u32 = 10;
pub const INTERVAL_CHOICES: [u32; 8] = [5, 10, 30, 60, 300, 600, 1_800, 3_600];
pub const ROUND_CHOICES: [u32; 7] = [0, 1, 5, 10, 25, 50, 100];
pub const STOP_CHOICES: [LoopStopWhen; 3] = [
    LoopStopWhen::ThreeEmpty,
    LoopStopWhen::RoundLimit,
    LoopStopWhen::ExplicitOff,
];

/// How often a composer with a draft tells the engine so (`pane.draft`): the engine forgets a
/// draft that is not repeated, and a loop never sends over one.
pub const DRAFT_BEAT: std::time::Duration =
    std::time::Duration::from_millis(soyeht_protocol::client_policy::DRAFT_BEAT_MS);

/// Whether the draft of a pane is due to be announced again: it has text, and it was never
/// announced or the last announcement is old.
pub fn draft_beat_due(
    has_draft: bool,
    last: Option<std::time::Instant>,
    now: std::time::Instant,
) -> bool {
    has_draft && last.is_none_or(|at| now.saturating_duration_since(at) >= DRAFT_BEAT)
}

pub fn interval_label(seconds: u32) -> String {
    if seconds < 60 {
        format!("{seconds} sec")
    } else {
        format!("{} min", seconds / 60)
    }
}

pub fn round_label(rounds: u32) -> String {
    if rounds == 0 {
        "no limit".to_owned()
    } else {
        format!("{rounds} rounds")
    }
}

/// The short name on the option's pill.
pub fn stop_label(stop_when: &LoopStopWhen) -> &'static str {
    match stop_when {
        LoopStopWhen::ThreeEmpty => "three empty",
        LoopStopWhen::RoundLimit => "round limit",
        LoopStopWhen::ExplicitOff => "until off",
    }
}

/// The name of the choice in the option's menu.
pub fn stop_choice_title(stop_when: &LoopStopWhen) -> &'static str {
    match stop_when {
        LoopStopWhen::ThreeEmpty => "Three empty results",
        LoopStopWhen::RoundLimit => "Round limit",
        LoopStopWhen::ExplicitOff => "Until turned off",
    }
}

/// The line in the footer: when the next tick is due and how many ticks ran today.
pub fn status(enabled: bool, pane_name: &str, rounds: u32) -> String {
    if enabled {
        format!("next: when {pane_name} is idle \u{b7} {rounds} today")
    } else {
        format!("off \u{b7} {rounds} today")
    }
}

/// What the panel edits: the pane's loop, else the defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub prompt: String,
    pub interval: u32,
    pub max_rounds: u32,
    pub stop_when: LoopStopWhen,
    pub enabled: bool,
}

impl Form {
    pub fn initial(config: Option<&LoopConfig>) -> Self {
        match config {
            Some(config) => Self {
                prompt: config.prompt.clone(),
                interval: config.interval,
                max_rounds: config.max_rounds,
                stop_when: config.stop_when.clone(),
                enabled: config.enabled,
            },
            None => Self {
                prompt: DEFAULT_PROMPT.to_owned(),
                interval: DEFAULT_INTERVAL,
                max_rounds: DEFAULT_MAX_ROUNDS,
                stop_when: LoopStopWhen::ThreeEmpty,
                enabled: false,
            },
        }
    }
}

/// Applying a loop that is on needs a prompt to send.
pub fn can_apply(enabled: bool, prompt: &str) -> bool {
    !(enabled && prompt.trim().is_empty())
}

impl State {
    /// The loop of a pane, when it has one.
    pub fn loop_of(&self, pane_id: &str) -> Option<&LoopConfig> {
        self.loops.get(pane_id)
    }

    /// The loops a snapshot lists, each with its pane.
    pub(crate) fn adopt_loops(&mut self, entries: &[Value]) {
        self.loops.clear();
        for entry in entries {
            let pane = entry.get("paneId").and_then(Value::as_str);
            let config = entry
                .get("config")
                .map(|config| serde_json::from_value::<LoopConfig>(config.clone()));
            match (pane, config) {
                (Some(pane), Some(Ok(config))) => {
                    self.loops.insert(pane.to_owned(), config);
                }
                _ => {
                    self.problem = Some("A pane's loop could not be read.".to_owned());
                }
            }
        }
    }

    /// The engine accepted a loop for a pane.
    pub(crate) fn set_loop(&mut self, pane_id: String, config: LoopConfig) {
        self.loops.insert(pane_id, config);
    }

    pub(crate) fn loop_notification(
        &mut self,
        name: EventName,
        params: Value,
    ) -> serde_json::Result<Vec<Effect>> {
        match name {
            EventName::LoopTick => {
                let event = serde_json::from_value::<LoopTickEvent>(params)?;
                if let Some(config) = self.loops.get_mut(&event.pane_id) {
                    config.rounds = Some(event.round);
                }
            }
            EventName::LoopStopped => {
                let event = serde_json::from_value::<LoopStoppedEvent>(params)?;
                if let Some(config) = self.loops.get_mut(&event.pane_id) {
                    config.enabled = false;
                }
            }
            _ => {}
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cases as listed, text, vector};
    use serde_json::json;

    fn cases(key: &str) -> Vec<Value> {
        listed("loop-panel", key)
    }

    fn number(value: &Value) -> u32 {
        u32::try_from(value.as_u64().unwrap()).unwrap()
    }

    fn stop(name: &Value) -> LoopStopWhen {
        serde_json::from_value(name.clone()).unwrap()
    }

    #[test]
    fn the_loop_panel_says_what_the_mac_says() {
        let all = vector("loop-panel");
        let words = &all["words"];
        for (held, key) in [
            (TITLE, "title"),
            (PROMPT_HEADING, "promptHeading"),
            (WHEN_HEADING, "whenHeading"),
            (EVERY_INTERVAL, "everyInterval"),
            (WHEN_IDLE, "whenIdle"),
            (WORKER_ROLE, "workerRole"),
            (LEAD_ROLE, "leadRole"),
            (CANCEL, "cancel"),
            (APPLY, "apply"),
        ] {
            assert_eq!(held, text(&words[key]), "{key}");
        }
        assert_eq!(words["rules"], json!(RULES));
        let defaults = Form::initial(None);
        assert_eq!(defaults.prompt, text(&all["defaults"]["prompt"]));
        assert_eq!(defaults.interval, number(&all["defaults"]["interval"]));
        assert_eq!(defaults.max_rounds, number(&all["defaults"]["maxRounds"]));
        assert_eq!(defaults.stop_when, stop(&all["defaults"]["stopWhen"]));
        assert!(!defaults.enabled);
        let intervals = cases("intervals");
        let held: Vec<u32> = intervals
            .iter()
            .map(|case| number(&case["seconds"]))
            .collect();
        assert_eq!(held, INTERVAL_CHOICES);
        for case in &intervals {
            assert_eq!(
                interval_label(number(&case["seconds"])),
                text(&case["label"])
            );
        }
        let rounds = cases("rounds");
        let held: Vec<u32> = rounds.iter().map(|case| number(&case["rounds"])).collect();
        assert_eq!(held, ROUND_CHOICES);
        for case in &rounds {
            assert_eq!(round_label(number(&case["rounds"])), text(&case["label"]));
        }
        let stops = cases("stops");
        let held: Vec<LoopStopWhen> = stops.iter().map(|case| stop(&case["stopWhen"])).collect();
        assert_eq!(held, STOP_CHOICES);
        for case in &stops {
            let value = stop(&case["stopWhen"]);
            assert_eq!(stop_label(&value), text(&case["label"]));
            assert_eq!(stop_choice_title(&value), text(&case["choice"]));
        }
        for case in cases("status") {
            let held = status(
                case["enabled"].as_bool().unwrap(),
                text(&case["paneName"]),
                number(&case["rounds"]),
            );
            assert_eq!(held, text(&case["text"]));
        }
        for case in cases("initial") {
            let config: Option<LoopConfig> =
                serde_json::from_value(case["config"].clone()).unwrap();
            let form = Form::initial(config.as_ref());
            assert_eq!(form.prompt, text(&case["prompt"]), "{case}");
            assert_eq!(form.interval, number(&case["interval"]));
            assert_eq!(form.max_rounds, number(&case["maxRounds"]));
            assert_eq!(form.stop_when, stop(&case["stopWhen"]));
            assert_eq!(form.enabled, case["enabled"].as_bool().unwrap());
        }
        for case in cases("canApply") {
            let held = can_apply(case["enabled"].as_bool().unwrap(), text(&case["prompt"]));
            assert_eq!(held, case["ok"].as_bool().unwrap(), "{case}");
        }
    }

    #[test]
    fn ticks_and_stops_change_the_loop_as_the_vector_says() {
        for case in cases("events") {
            let mut state = State::default();
            let config: LoopConfig = serde_json::from_value(case["config"].clone()).unwrap();
            state.set_loop("p".into(), config);
            let mut params = case["params"].clone();
            params["eventId"] = json!("1");
            params["at"] = json!("2026-10-11T12:00:00Z");
            let name: EventName = serde_json::from_value(case["event"].clone()).unwrap();
            state.loop_notification(name, params).unwrap();
            let held = state.loop_of("p").unwrap();
            assert_eq!(held.rounds, Some(number(&case["rounds"])), "{case}");
            assert_eq!(held.enabled, case["enabled"].as_bool().unwrap());
        }
    }

    #[test]
    fn a_draft_is_announced_when_it_starts_and_every_few_seconds_while_it_stays() {
        let start = std::time::Instant::now();
        let later = |seconds| start + std::time::Duration::from_secs(seconds);
        assert!(draft_beat_due(true, None, start));
        assert!(!draft_beat_due(true, Some(start), later(7)));
        assert!(draft_beat_due(true, Some(start), later(8)));
        assert!(!draft_beat_due(false, None, later(100)));
    }

    #[test]
    fn a_snapshot_lists_the_loops_with_their_panes() {
        let mut state = State::default();
        let config = json!({"prompt": "go", "interval": 60, "maxRounds": 3, "stopWhen": "round_limit", "enabled": true});
        state.adopt_loops(&[json!({"paneId": "a", "config": config})]);
        assert_eq!(state.loop_of("a").unwrap().prompt, "go");
        assert!(state.loop_of("b").is_none() && state.problem.is_none());
        state.adopt_loops(&[json!({"config": config})]);
        assert!(state.loop_of("a").is_none() && state.problem.is_some());
    }
}
