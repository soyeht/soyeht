//! Skills the program carries as text, for the engine to place where an agent can load them.
//! They are ordinary `SKILL.md` files; this module only holds them.

/// How to build the frames of an avatar with the image catalogue and `soyeht avatar` (ADR 0166).
pub const AVATAR_BUILDER: &str = include_str!("../skills/avatar-builder/SKILL.md");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn the_avatar_builder_is_a_valid_skill_that_names_the_commands_it_relies_on() {
        let manifest = parse("avatar-builder", AVATAR_BUILDER).unwrap();
        assert_eq!(manifest.name, "avatar-builder");
        for command in [
            "soyeht avatar providers",
            "soyeht avatar frames",
            "soyeht avatar frame ",
            "soyeht avatar check ",
            "soyeht avatar key set",
        ] {
            assert!(AVATAR_BUILDER.contains(command), "{command}");
        }
    }
}
