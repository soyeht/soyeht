#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_image::{Error, spec};
use std::collections::BTreeSet;

#[test]
fn the_spec_is_the_eighteen_frames_of_the_package() {
    let spec = spec().unwrap();
    assert_eq!(spec.frames.len(), 18);
    let ids: BTreeSet<_> = spec.frames.iter().map(|frame| frame.id.as_str()).collect();
    assert_eq!(ids.len(), 18, "ids are unique");
    let count = |group: &str| {
        spec.frames
            .iter()
            .filter(|frame| frame.group == group)
            .count()
    };
    assert_eq!(
        [
            count("mouth"),
            count("blink"),
            count("eyes"),
            count("resting")
        ],
        [8, 1, 7, 2]
    );
}

#[test]
fn every_group_has_a_template_that_names_the_change_and_what_stays() {
    let spec = spec().unwrap();
    for (name, group) in &spec.groups {
        assert!(group.template.contains("{change}"), "{name}");
        assert!(group.template.contains("{keep}"), "{name}");
        assert!(
            group.min_change > 0.0 && group.min_change < group.max_change,
            "{name}"
        );
        assert!(group.max_change < 1.0, "{name}");
    }
}

#[test]
fn a_prompt_holds_the_change_and_the_correction_and_no_placeholder() {
    let spec = spec().unwrap();
    for frame in &spec.frames {
        let prompt = spec.prompt(frame, None).unwrap();
        assert!(prompt.contains(&frame.change), "{}", frame.id);
        assert!(
            !prompt.contains('{') && !prompt.contains('}'),
            "{}",
            frame.id
        );
    }
    let frame = spec.frame("A").unwrap();
    let prompt = spec.prompt(frame, Some("  The head stays put. ")).unwrap();
    assert!(prompt.ends_with(" The head stays put."));
    assert_eq!(
        spec.prompt(frame, Some("   ")).unwrap(),
        spec.prompt(frame, None).unwrap()
    );
}

#[test]
fn an_unknown_frame_is_named() {
    assert!(matches!(
        spec().unwrap().frame("zzz"),
        Err(Error::UnknownFrame(id)) if id == "zzz"
    ));
}
