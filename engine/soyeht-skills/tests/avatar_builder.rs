#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The `avatar-builder` skill (ADR 0166, slice 9a) is a plain `SKILL.md` that the build job puts
//! where an agent loads it. It must stay a valid skill and keep naming the commands it relies on.

const FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skills/avatar-builder/SKILL.md"
);

#[test]
fn the_avatar_builder_is_a_valid_skill_that_names_the_commands_it_relies_on() {
    let text = std::fs::read_to_string(FILE).unwrap();
    let manifest = soyeht_skills::parse("avatar-builder", &text).unwrap();
    assert_eq!(manifest.name, "avatar-builder");
    for command in [
        "soyeht avatar providers",
        "soyeht avatar frames",
        "soyeht avatar frame ",
        "soyeht avatar check ",
        "soyeht avatar key set",
    ] {
        assert!(text.contains(command), "{command}");
    }
}
