use crate::{Confidence, Detection, Detector, Error, ObservationFrame, Result};
use regex::Regex;
use soyeht_protocol::{PaneState, StateSource};

/// The single authored-rule control used to evaluate later detector candidates.
///
/// This baseline is evidence for comparison, not a selected production detector.
pub struct RegexBaseline {
    rules: Vec<Rule>,
}

struct Rule {
    id: &'static str,
    priority: u16,
    harness: HarnessFamily,
    target: Target,
    pattern: Regex,
    state: PaneState,
    source: StateSource,
    confidence: Confidence,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HarnessFamily {
    Claude,
    Codex,
    OpenCode,
}

#[derive(Clone, Copy)]
enum Target {
    Title,
    Screen,
}

macro_rules! specification {
    ($id:expr, $priority:expr, $harness:expr, $target:expr, $pattern:expr, $state:expr, $source:expr, $confidence:expr $(,)?) => {
        Specification {
            id: $id,
            priority: $priority,
            harness: $harness,
            target: $target,
            pattern: $pattern,
            state: $state,
            source: $source,
            confidence: $confidence,
        }
    };
}

impl RegexBaseline {
    pub fn new() -> Result<Self> {
        let specifications = claude_specifications()
            .into_iter()
            .chain(codex_specifications())
            .chain(opencode_specifications());
        let mut rules = Vec::new();
        for specification in specifications {
            rules.push(specification.build()?);
        }
        Ok(Self { rules })
    }
}

fn claude_specifications() -> [Specification; 6] {
    [
        specification!(
            "claude_title_waiting",
            1_100,
            HarnessFamily::Claude,
            Target::Title,
            r"(?i)action required",
            PaneState::WaitingForYou,
            StateSource::Osc,
            9_800
        ),
        specification!(
            "claude_title_working",
            1_050,
            HarnessFamily::Claude,
            Target::Title,
            r"^[\x{2800}-\x{28ff}\x{25d0}-\x{25d3}] ",
            PaneState::Working,
            StateSource::Osc,
            9_700
        ),
        specification!(
            "claude_prompt_idle",
            950,
            HarnessFamily::Claude,
            Target::Screen,
            r"(?m)^\s*❯\s*$",
            PaneState::Idle,
            StateSource::Screen,
            8_500
        ),
        specification!(
            "claude_screen_waiting",
            840,
            HarnessFamily::Claude,
            Target::Screen,
            r"(?is)(esc to cancel.{0,240}(enter to confirm|enter to select)|do you want to proceed\?.{0,600}\byes\b.{0,600}\bno\b|waiting for permission|requests your input)",
            PaneState::WaitingForYou,
            StateSource::Screen,
            9_500
        ),
        specification!(
            "claude_screen_working",
            500,
            HarnessFamily::Claude,
            Target::Screen,
            r"(?im)(esc to interrupt|waiting for [1-9][0-9]* background agents? to finish)",
            PaneState::Working,
            StateSource::Screen,
            9_000
        ),
        specification!(
            "claude_title_idle",
            100,
            HarnessFamily::Claude,
            Target::Title,
            r"^\x{2733} ",
            PaneState::Idle,
            StateSource::Osc,
            9_500
        ),
    ]
}

fn codex_specifications() -> [Specification; 5] {
    [
        specification!(
            "codex_title_waiting",
            1_100,
            HarnessFamily::Codex,
            Target::Title,
            r"(?i)action required",
            PaneState::WaitingForYou,
            StateSource::Osc,
            9_800
        ),
        specification!(
            "codex_title_working",
            1_050,
            HarnessFamily::Codex,
            Target::Title,
            r"(?:^| )[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏](?: |$)",
            PaneState::Working,
            StateSource::Osc,
            9_700
        ),
        specification!(
            "codex_prompt_idle",
            950,
            HarnessFamily::Codex,
            Target::Screen,
            r"(?m)^\s*›\s*$",
            PaneState::Idle,
            StateSource::Screen,
            8_500
        ),
        specification!(
            "codex_screen_waiting",
            900,
            HarnessFamily::Codex,
            Target::Screen,
            r"(?is)(press enter to confirm or esc to cancel|enter to submit (answer|all)|allow command\?|do you trust the contents of this directory\?)",
            PaneState::WaitingForYou,
            StateSource::Screen,
            9_500
        ),
        specification!(
            "codex_screen_working",
            500,
            HarnessFamily::Codex,
            Target::Screen,
            r"(?im)^[•◦]\s+working \([^)]*esc to interrupt\)(?: · .*)?$",
            PaneState::Working,
            StateSource::Screen,
            9_000
        ),
    ]
}

fn opencode_specifications() -> [Specification; 3] {
    [
        specification!(
            "opencode_prompt_idle",
            50,
            HarnessFamily::OpenCode,
            Target::Screen,
            r"(?m)^\s*>\s*$",
            PaneState::Idle,
            StateSource::Screen,
            8_000
        ),
        specification!(
            "opencode_screen_waiting",
            300,
            HarnessFamily::OpenCode,
            Target::Screen,
            r"(?is)(△ permission required|esc dismiss.{0,240}(enter confirm|enter submit|enter toggle))",
            PaneState::WaitingForYou,
            StateSource::Screen,
            9_500
        ),
        specification!(
            "opencode_screen_working",
            100,
            HarnessFamily::OpenCode,
            Target::Screen,
            r"(?im)(esc to interrupt|ctrl\+c to interrupt|press esc to interrupt|(■|⬝){4,}|retrying in [0-9]+s)",
            PaneState::Working,
            StateSource::Screen,
            9_000
        ),
    ]
}

impl Detector for RegexBaseline {
    fn state(&self, frame: &ObservationFrame) -> Detection {
        let Some(harness) = frame.harness.id().and_then(harness_family) else {
            return Detection::unknown();
        };
        // Terminal emulators expose unwritten cells as NULs. They render blank,
        // and treating them as bytes would split otherwise whole
        // permission footers and create a false unknown state.
        let screen = frame.lines.join("\n").replace('\0', " ");
        let mut winner: Option<&Rule> = None;
        for rule in &self.rules {
            if rule.harness != harness {
                continue;
            }
            let value = match rule.target {
                Target::Title => frame.osc_title.as_deref().unwrap_or_default(),
                Target::Screen => &screen,
            };
            if rule.pattern.is_match(value)
                && winner.is_none_or(|current| rule.priority > current.priority)
            {
                winner = Some(rule);
            }
        }
        winner.map_or_else(Detection::unknown, |rule| Detection {
            state: rule.state.clone(),
            source: rule.source.clone(),
            confidence: rule.confidence,
            rule: Some(rule.id),
        })
    }
}

struct Specification {
    id: &'static str,
    priority: u16,
    harness: HarnessFamily,
    target: Target,
    pattern: &'static str,
    state: PaneState,
    source: StateSource,
    confidence: u16,
}

impl Specification {
    fn build(self) -> Result<Rule> {
        Ok(Rule {
            id: self.id,
            priority: self.priority,
            harness: self.harness,
            target: self.target,
            pattern: Regex::new(self.pattern).map_err(|_| Error::Pattern(self.id.into()))?,
            state: self.state,
            source: self.source,
            confidence: Confidence::from_basis_points(self.confidence)?,
        })
    }
}

fn harness_family(id: &str) -> Option<HarnessFamily> {
    match id.to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "registry.claude-acp" => Some(HarnessFamily::Claude),
        "codex" | "registry.codex-acp" => Some(HarnessFamily::Codex),
        "opencode" | "registry.opencode" => Some(HarnessFamily::OpenCode),
        _ => None,
    }
}
