#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_editor::{Error, rank_paths, search_content};

#[test]
fn literal_search_preserves_unicode_ranges_and_metacharacters() {
    let text = "one .*\n😀 .* and .*\nfinal";
    let exact = search_content(text, ".*", None, true, 500).unwrap();
    assert_eq!(exact.matches.len(), 3);
    assert_eq!(exact.matches[1].range.start.line, 1);
    assert_eq!(exact.matches[1].range.start.column, 3);
    assert_eq!(exact.matches[1].range.end.column, 5);
    assert_eq!(exact.matches[1].preview, "😀 .* and .*");

    let folded = search_content("Alpha alpha ALPHA", "alpha", None, false, 500).unwrap();
    assert_eq!(folded.matches.len(), 3);
}

#[test]
fn literal_replacement_is_bounded_and_empty_queries_fail_closed() {
    let result = search_content("x x x", "x", Some("long"), true, 2).unwrap();
    assert_eq!(result.replaced, 2);
    assert_eq!(result.replacement.as_deref(), Some("long long x"));
    assert_eq!(result.matches.len(), 2);
    assert_eq!(
        search_content("anything", "", None, true, 1),
        Err(Error::EmptySearch)
    );

    let many = format!("{} needle", "z".repeat(5_000));
    let capped = search_content(&many, "z", None, true, usize::MAX).unwrap();
    assert_eq!(
        capped.matches.len(),
        soyeht_protocol::limits::EDITOR_SEARCH_RESULTS
    );
    assert!(
        capped
            .matches
            .iter()
            .all(|item| item.preview.chars().count() <= 4_096)
    );
}

#[test]
fn fuzzy_path_search_is_deterministic_and_reports_utf16_spans() {
    let paths = vec![
        "src/workspace_model.rs".to_owned(),
        "src/workspace.rs".to_owned(),
        "tests/work_space.rs".to_owned(),
        "src/😀workspace.swift".to_owned(),
        "unrelated.txt".to_owned(),
    ];
    let matches = rank_paths(&paths, "ws", 3).unwrap();
    assert_eq!(matches.len(), 3);
    assert_eq!(matches[0].path, "tests/work_space.rs");
    assert!(
        matches
            .windows(2)
            .all(|pair| pair[0].score >= pair[1].score)
    );

    let emoji = rank_paths(&paths, "😀w", 5)
        .unwrap()
        .into_iter()
        .find(|item| item.path.contains('😀'))
        .unwrap();
    assert_eq!(emoji.range.start.column, 4);
    assert_eq!(emoji.range.end.column, 7);
}
