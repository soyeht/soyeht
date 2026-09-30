//! The one rule for recognizing an agent's permission-mode selector (ADR 0070).

use crate::ConfigOption;

/// The ACP configuration category that carries an agent's permission modes.
pub const MODE_CATEGORY: &str = "mode";

impl ConfigOption {
    /// Whether this option is the agent's own permission-mode selector.
    pub fn is_permission_mode(&self) -> bool {
        self.category.as_deref() == Some(MODE_CATEGORY) || self.id == MODE_CATEGORY
    }
}
