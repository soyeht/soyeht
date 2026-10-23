//! The lists of documents a section shows, and making a new document.

use super::*;

fn listing(entries: &[(&str, &str)], next: Option<&str>) -> Value {
    json!({
        "entries": entries.iter().map(|(path, title)| json!({
            "path": path, "title": title, "kind": "text", "section": "texts",
            "modifiedAt": "", "byteCount": "10"
        })).collect::<Vec<_>>(),
        "nextCursor": next
    })
}

fn listed(cursor: Option<&str>, result: Value) -> Event {
    Event::Writing(WritingEvent::Listed {
        workspace_id: "w".into(),
        section: WritingSection::Texts,
        cursor: cursor.map(str::to_owned),
        result: serde_json::from_value(result).unwrap(),
    })
}

fn paths(state: &State) -> Vec<String> {
    state
        .writings
        .section("w", &WritingSection::Texts)
        .map(|list| {
            list.entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_section_is_listed_a_page_at_a_time() {
    let mut state = State::default();
    let first = sent_writing(&state.writing_list("w", &WritingSection::Texts, false));
    assert_eq!(first.len(), 1);
    // A page on its way is not asked for twice.
    assert!(
        state
            .writing_list("w", &WritingSection::Texts, false)
            .is_empty()
    );
    state.apply(listed(
        None,
        listing(&[("texts/a.md", "A"), ("texts/b.md", "B")], Some("c1")),
    ));
    assert_eq!(paths(&state), ["texts/a.md", "texts/b.md"]);
    let more = sent_writing(&state.writing_list("w", &WritingSection::Texts, true));
    assert_eq!(
        more,
        vec![WritingCommand::List {
            workspace_id: "w".into(),
            section: WritingSection::Texts,
            cursor: Some("c1".into())
        }]
    );
    state.apply(listed(
        Some("c1"),
        listing(&[("texts/b.md", "B"), ("texts/c.md", "C")], None),
    ));
    assert_eq!(paths(&state), ["texts/a.md", "texts/b.md", "texts/c.md"]);
    assert!(
        state
            .writing_list("w", &WritingSection::Texts, true)
            .is_empty(),
        "there is no next page"
    );
}

#[test]
fn only_the_documents_of_the_sections_own_kind_are_listed() {
    let mut state = State::default();
    let mut result = listing(&[("texts/a.md", "A")], None);
    result["entries"].as_array_mut().unwrap().push(json!({
        "path": "texts/deck.md", "title": "Deck", "kind": "presentation", "section": "texts",
        "modifiedAt": "", "byteCount": "1"
    }));
    state.apply(listed(None, result));
    assert_eq!(paths(&state), ["texts/a.md"]);
}

#[test]
fn a_list_that_failed_says_so_and_can_be_asked_again() {
    let mut state = State::default();
    state.writing_list("w", &WritingSection::Texts, false);
    state.apply(Event::Writing(WritingEvent::Failed(Failure::List {
        workspace_id: "w".into(),
        section: WritingSection::Texts,
    })));
    let list = state.writings.section("w", &WritingSection::Texts).unwrap();
    assert!(list.failed && !list.loading);
    assert_eq!(
        sent_writing(&state.writing_list("w", &WritingSection::Texts, false)).len(),
        1
    );
}

fn opened(revision: u64) -> Event {
    let pane = writing_pane();
    let viewport: FileReadResult = serde_json::from_value(json!({
        "file": file(revision, 1, false), "source": "buffer", "fromLine": 0, "toLine": 1,
        "lines": [{"line": 0, "text": "fresh", "highlights": []}]
    }))
    .unwrap();
    Event::Writing(WritingEvent::Opened(Box::new(Opened {
        document: document(revision, 1),
        pane,
        viewport,
    })))
}

#[test]
fn a_new_document_is_made_once_and_opens_in_its_pane() {
    let mut state = empty();
    let draft = DocumentDraft {
        title: "  Essay ".into(),
        folder: String::new(),
    };
    let asked = sent_writing(&state.writing_create("w", &WritingSection::Texts, &draft));
    assert_eq!(
        asked,
        vec![WritingCommand::Create {
            workspace_id: "w".into(),
            section: WritingSection::Texts,
            submission: Submission {
                title: "Essay".into(),
                relative_folder: None
            }
        }]
    );
    // Return again while the engine answers asks for nothing.
    assert!(
        state
            .writing_create("w", &WritingSection::Texts, &draft)
            .is_empty()
    );
    let effects = state.apply(opened(1));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::FocusPane(pane) if pane == "p1"))
    );
    assert_eq!(whole_text(&state).as_deref(), Some("fresh"));
    assert!(state.writings.of_pane("p1").is_some());
    assert!(
        !state
            .writings
            .section("w", &WritingSection::Texts)
            .unwrap()
            .creating
    );
}

#[test]
fn a_blank_title_asks_for_nothing_and_a_refused_creation_can_be_tried_again() {
    let mut state = State::default();
    let blank = DocumentDraft::default();
    assert!(
        state
            .writing_create("w", &WritingSection::Texts, &blank)
            .is_empty()
    );
    let draft = DocumentDraft {
        title: "Essay".into(),
        folder: String::new(),
    };
    state.writing_create("w", &WritingSection::Texts, &draft);
    state.apply(Event::Writing(WritingEvent::Failed(Failure::Create {
        workspace_id: "w".into(),
        section: WritingSection::Texts,
    })));
    assert_eq!(
        sent_writing(&state.writing_create("w", &WritingSection::Texts, &draft)).len(),
        1
    );
}
