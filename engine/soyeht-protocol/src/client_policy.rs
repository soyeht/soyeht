// Generated from contracts/protocol.json. Do not edit.

use crate::{ConfigOption, HighlightKind, PaneState, UsageProvider};

pub const REQUEST_TIMEOUT_MS: u64 = 12000;
pub const GREETING_TIMEOUT_MS: u64 = 5000;
pub const RECONNECT_DELAY_MS: u64 = 1000;
pub const RECONNECT_CEILING_MS: u64 = 30000;
pub const DRAFT_BEAT_MS: u64 = 8000;
pub const TERMINAL_INPUT_CHUNK_BYTES: usize = 16384;
pub const PAGE_SIZE_QUEUE: u32 = 64;
pub const PAGE_SIZE_HARNESSES: u32 = 64;
pub const PAGE_SIZE_MODELS: u32 = 64;
pub const PAGE_SIZE_TRANSCRIPT: u32 = 100;
pub const PAGE_SIZE_TASKS: u32 = 100;
pub const HARNESS_SWITCH_TITLE: &str = "Switch agent";
pub const HARNESS_SWITCH_HANDOVER: &str = "This starts a new agent session. Recent messages and pending prompts are handed over as historical data.";
pub const TASK_MOVE_REASON: &str = "Moved on the workspace board";
pub const EDITOR_NO_FILE: &str = "The engine has no file for this editor tab.";
pub const USAGE_BAND_WARNING_FROM: f64 = 0.5;
pub const USAGE_BAND_DANGER_ABOVE: f64 = 0.8;
pub const CONTEXT_BAND_WARNING_ABOVE: f64 = 0.75;
pub const CONTEXT_BAND_DANGER_ABOVE: f64 = 0.9;
pub const USAGE_RING_LAST_KNOWN_ARC_OPACITY: f64 = 0.45;
pub const USAGE_RING_UNAVAILABLE_OPACITY: f64 = 0.46;
pub const USAGE_WINDOW_WEEKLY_KEYWORD: &str = "week";
pub const EDITOR_PREFETCH_LINES: u32 = 160;
pub const EDITOR_PAGE_LEAD_DIVISOR: u32 = 4;
pub const EDITOR_TAB_WIDTH: u32 = 4;
pub const EDITOR_TAB_HISTORY: u32 = 64;
pub const EDITOR_SAVE_ATTEMPTS: u32 = 30;
pub const EDITOR_SAVE_RETRY_MS: u32 = 100;
pub const EDITOR_INDENT_UNIT: &str = "    ";
pub const EDITOR_INDENT_OPENERS: &str = "{[(";
/// Image types that claude-agent-acp and codex-acp forward as model input and that both
/// providers accept; any other image is linked.
pub const INLINE_IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
pub const MODEL_CATEGORY: &str = "model";
pub const EFFORT_CATEGORY: &str = "thought_level";
pub const PERMISSION_MODE_CATEGORY: &str = "mode";

/// The palette role a pane state's dot is colored from; each client resolves the role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateDot {
    Success,
    Accent,
    Warning,
    Muted,
    Secondary,
}

impl PaneState {
    /// The wire spelling of the state (`waiting_for_you`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Working => "working",
            Self::Idle => "idle",
            Self::WaitingForYou => "waiting_for_you",
            Self::Cancelling => "cancelling",
            Self::NoSignal => "no_signal",
            Self::Recovering => "recovering",
            Self::Dead => "dead",
            Self::Unknown => "unknown",
        }
    }

    /// Whether an agent in this state is in the middle of a turn: it can be stopped, what
    /// is typed is queued, and the header counts the time.
    pub fn is_turn_running(&self) -> bool {
        matches!(
            self,
            Self::Working | Self::WaitingForYou | Self::Cancelling | Self::NoSignal
        )
    }

    /// The role of the state's dot.
    pub fn dot(&self) -> StateDot {
        match self {
            Self::Starting => StateDot::Secondary,
            Self::Working => StateDot::Success,
            Self::Idle => StateDot::Success,
            Self::WaitingForYou => StateDot::Accent,
            Self::Cancelling => StateDot::Secondary,
            Self::NoSignal => StateDot::Warning,
            Self::Recovering => StateDot::Secondary,
            Self::Dead => StateDot::Muted,
            Self::Unknown => StateDot::Muted,
        }
    }
}

/// The character an editor types after `opening` to close the pair, when it closes one
/// (brackets and quotes).
pub fn editor_closing(opening: &str) -> Option<&'static str> {
    match opening {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        "\"" => Some("\""),
        "'" => Some("'"),
        _ => None,
    }
}

/// The palette role a syntax highlight is colored from; each client resolves the role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightRole {
    Accent,
    SyntaxType,
    Warning,
    Muted,
    Success,
    Ink,
}

impl HighlightKind {
    /// The role of the highlight's color.
    pub fn role(&self) -> HighlightRole {
        match self {
            Self::Keyword => HighlightRole::Accent,
            Self::String => HighlightRole::Success,
            Self::Comment => HighlightRole::Muted,
            Self::Number => HighlightRole::Warning,
            Self::Type => HighlightRole::SyntaxType,
            Self::Function => HighlightRole::Accent,
            Self::Property => HighlightRole::Ink,
            Self::Operator => HighlightRole::Accent,
            Self::Punctuation => HighlightRole::Ink,
            Self::Constant => HighlightRole::Warning,
            Self::Variable => HighlightRole::Ink,
        }
    }
}

impl UsageProvider {
    /// The name the clients show for the provider.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
        }
    }
}

impl ConfigOption {
    // A selector has a role when its category (its id when it has none) names it.

    /// Whether this option is the agent's model selector.
    pub fn is_model(&self) -> bool {
        self.category.as_deref().unwrap_or(&self.id) == MODEL_CATEGORY
    }

    /// Whether this option is the agent's effort selector.
    pub fn is_effort(&self) -> bool {
        self.category.as_deref().unwrap_or(&self.id) == EFFORT_CATEGORY || self.id == "effort"
    }

    /// Whether this option is the agent's permission selector.
    pub fn is_permission_mode(&self) -> bool {
        self.category.as_deref().unwrap_or(&self.id) == PERMISSION_MODE_CATEGORY
            || self.id == "mode"
    }
}
