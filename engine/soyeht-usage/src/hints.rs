//! What the caller knows that a reader cannot find on the device.

use crate::sources::opencode;

/// Facts the engine copies under its lock and hands to a refresh. A reader uses a hint before it
/// looks at the files of the program it reads, and a hint it does not need is ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadHints {
    /// The model (`provider/model`) the most recently active OpenCode pane uses. OpenCode's own
    /// configuration only says what it starts with: a pane can be switched to another provider.
    pub opencode_model: Option<String>,
}

impl ReadHints {
    /// The provider the OpenCode pane's model belongs to, when the hint names one.
    pub fn opencode_provider(&self) -> Option<&str> {
        self.opencode_model
            .as_deref()
            .and_then(opencode::provider_of)
    }
}

#[cfg(test)]
mod tests {
    use super::ReadHints;

    fn hint(model: &str) -> ReadHints {
        ReadHints {
            opencode_model: Some(model.to_owned()),
        }
    }

    #[test]
    fn the_provider_is_the_part_of_the_model_before_the_first_slash() {
        assert_eq!(
            hint("opencode-go/glm-5.3-flash").opencode_provider(),
            Some("opencode-go")
        );
        assert_eq!(hint(" a/b/c ").opencode_provider(), Some("a"));
    }

    #[test]
    fn a_model_with_no_provider_is_no_hint() {
        assert_eq!(hint("glm-5.3-flash").opencode_provider(), None);
        assert_eq!(hint("/glm").opencode_provider(), None);
        assert_eq!(
            hint(&format!("{}/m", "p".repeat(129))).opencode_provider(),
            None
        );
        assert_eq!(ReadHints::default().opencode_provider(), None);
    }
}
