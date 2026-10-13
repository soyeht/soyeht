// Generated from contracts/protocol.json (sha256 96e9a6c2600dc2ec7c40a94f545618ee858e5ce15ee105719d8c3c8114ec5404). Do not edit.

use crate::{ConfigOption, UsageProvider};

pub const REQUEST_TIMEOUT_MS: u64 = 12000;
pub const GREETING_TIMEOUT_MS: u64 = 5000;
pub const RECONNECT_DELAY_MS: u64 = 1000;
pub const TERMINAL_INPUT_CHUNK_BYTES: usize = 16384;
pub const USAGE_BAND_WARNING_FROM: f64 = 0.5;
pub const USAGE_BAND_DANGER_ABOVE: f64 = 0.8;
pub const CONTEXT_BAND_WARNING_ABOVE: f64 = 0.75;
pub const CONTEXT_BAND_DANGER_ABOVE: f64 = 0.9;
pub const USAGE_RING_LAST_KNOWN_ARC_OPACITY: f64 = 0.45;
pub const USAGE_RING_UNAVAILABLE_OPACITY: f64 = 0.46;
pub const USAGE_WINDOW_WEEKLY_KEYWORD: &str = "week";
pub const MODEL_CATEGORY: &str = "model";
pub const EFFORT_CATEGORY: &str = "thought_level";
pub const PERMISSION_MODE_CATEGORY: &str = "mode";

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
