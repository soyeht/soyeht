//! The slides of a presentation pane, as the engine's structure gives them.

use super::{document, open};
use crate::{Event, writing::WritingEvent};
use serde_json::json;
use soyeht_protocol::WritingStructureResult;

fn structure_with_slides(revision: u64) -> Event {
    Event::Writing(WritingEvent::Structure(Box::new(
        serde_json::from_value::<WritingStructureResult>(json!({
            "document": serde_json::to_value(document(revision, 3)).unwrap(),
            "slides": [
                {"index": 0, "startLine": 0, "endLine": 1, "heading": "One", "notes": "",
                 "imagePaths": []},
                {"index": 1, "startLine": 2, "endLine": 3, "heading": "Two", "notes": "Say it",
                 "imagePaths": ["a.png"]}
            ],
            "scenes": [], "elements": [], "variants": [], "beats": [],
            "wordCount": 2, "truncated": false
        }))
        .unwrap(),
    )))
}

#[test]
fn the_slides_come_with_the_count_and_only_after_it() {
    let mut state = open(&["# One", "---", "# Two"]);
    assert!(state.writings.of_pane("p1").unwrap().slides().is_none());
    state.apply(structure_with_slides(1));
    let slides = state.writings.of_pane("p1").unwrap().slides().unwrap();
    assert_eq!(slides.len(), 2);
    assert_eq!(slides[1].heading, "Two");
    assert_eq!(slides[1].image_paths, ["a.png"]);
}
