#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use soyeht_editor::{
    ContentMatcher, EditHistory, EditStep, Error, FindDirection, FindOutcome, FindRequest,
    HistoryDirection, MatchOptions, TextBuffer, find, replace_all, replace_one,
};
use soyeht_protocol::{TextPosition, TextRange};
use std::time::{Duration, Instant};
use support::position;

fn matcher(query: &str, case_sensitive: bool, whole_word: bool, regex: bool) -> ContentMatcher {
    ContentMatcher::with_options(
        query,
        MatchOptions {
            case_sensitive,
            whole_word,
            regex,
        },
    )
    .unwrap()
}

fn search(
    text: &str,
    matcher: &ContentMatcher,
    from: TextPosition,
    direction: FindDirection,
) -> FindOutcome {
    find(
        text,
        &FindRequest {
            matcher,
            from,
            direction,
            window: 0..u32::MAX,
            visible_limit: 500,
        },
    )
}

fn range(start: (u32, u32), end: (u32, u32)) -> TextRange {
    TextRange {
        start: position(start.0, start.1),
        end: position(end.0, end.1),
    }
}

#[test]
fn options_cover_case_whole_word_and_regex() {
    let text = "Foo foo food _foo\nfoo_bar FOO";
    let count =
        |matcher: ContentMatcher| search(text, &matcher, position(0, 0), FindDirection::Next).total;
    assert_eq!(count(matcher("foo", false, false, false)), 6);
    assert_eq!(count(matcher("foo", true, false, false)), 4);
    assert_eq!(count(matcher("foo", false, true, false)), 3);
    assert_eq!(count(matcher("f[o]+d?", true, false, true)), 4);
    assert_eq!(count(matcher("^foo", false, false, true)), 2);
    assert_eq!(count(matcher("FOO$", true, false, true)), 1);
    assert_eq!(count(matcher("x*", false, false, true)), 0);
    assert_eq!(
        ContentMatcher::with_options(
            "(",
            MatchOptions {
                regex: true,
                ..MatchOptions::default()
            }
        )
        .err(),
        Some(Error::SearchPattern)
    );
    // Literal mode keeps metacharacters literal.
    assert_eq!(count(matcher("f[o]+", true, false, false)), 0);
}

#[test]
fn next_and_previous_step_through_every_match_and_wrap() {
    let text = "a needle\nneedle 😀 needle\nend";
    let needle = matcher("needle", true, false, false);
    let first = search(text, &needle, position(0, 0), FindDirection::Next);
    assert_eq!(first.total, 3);
    let current = first.current.unwrap();
    assert_eq!(
        (current.index, current.range.clone()),
        (0, range((0, 2), (0, 8)))
    );

    let second = search(text, &needle, current.range.end, FindDirection::Next);
    assert_eq!(second.current.unwrap().range, range((1, 0), (1, 6)));
    let third = search(text, &needle, position(1, 6), FindDirection::Next);
    let third = third.current.unwrap();
    assert_eq!((third.index, third.range), (2, range((1, 10), (1, 16))));
    let wrapped = search(text, &needle, position(2, 3), FindDirection::Next);
    assert_eq!(wrapped.current.unwrap().index, 0);

    let back = search(text, &needle, position(1, 10), FindDirection::Previous);
    assert_eq!(back.current.unwrap().index, 1);
    let wrapped_back = search(text, &needle, position(0, 2), FindDirection::Previous);
    assert_eq!(wrapped_back.current.unwrap().index, 2);
    // Positions past the end of a line or the document clamp instead of failing.
    let clamped = search(text, &needle, position(0, 400), FindDirection::Next);
    assert_eq!(clamped.current.unwrap().index, 1);
    let past_end = search(text, &needle, position(90, 0), FindDirection::Previous);
    assert_eq!(past_end.current.unwrap().index, 2);
    let none = search(
        text,
        &matcher("absent", true, false, false),
        position(0, 0),
        FindDirection::Next,
    );
    assert_eq!((none.total, none.current), (0, None));
}

#[test]
fn visible_matches_stay_inside_the_window_and_the_limit() {
    let text = (0..10)
        .map(|line| format!("x{line} x"))
        .collect::<Vec<_>>()
        .join("\n");
    let needle = matcher("x", true, false, false);
    let outcome = find(
        &text,
        &FindRequest {
            matcher: &needle,
            from: position(0, 0),
            direction: FindDirection::Next,
            window: 4..6,
            visible_limit: 3,
        },
    );
    assert_eq!(outcome.total, 20);
    assert_eq!(
        outcome.visible,
        vec![
            range((4, 0), (4, 1)),
            range((4, 3), (4, 4)),
            range((5, 0), (5, 1))
        ]
    );
}

#[test]
fn replace_one_requires_a_live_match_and_expands_captures() {
    let text = "let alpha = beta;\nlet gamma = delta;";
    let assignment = matcher(r"let (\w+) = (\w+)", true, false, true);
    let edit = replace_one(text, &assignment, &range((1, 0), (1, 17)), "let $2 = $1").unwrap();
    assert_eq!(edit.text, "let delta = gamma");
    assert!(replace_one(text, &assignment, &range((1, 1), (1, 17)), "x").is_none());
    assert!(replace_one(text, &assignment, &range((5, 0), (5, 3)), "x").is_none());

    let literal = matcher("$1", true, false, false);
    let literal_edit = replace_one("cost $1", &literal, &range((0, 5), (0, 7)), "$2").unwrap();
    assert_eq!(literal_edit.text, "$2");
}

#[test]
fn replace_all_is_one_bounded_undo_step() {
    let mut text = String::new();
    for block in 0..40 {
        text.push_str(&format!("old {block} old\n"));
        text.push_str(&"padding line\n".repeat(400));
    }
    let old = matcher("old", true, true, false);
    let (edits, replaced) = replace_all(&text, &old, "new").unwrap();
    assert_eq!(replaced, 80);
    assert_eq!(edits.len(), 1, "40 far-apart clusters fold into one span");

    let mut buffer = TextBuffer::new(text.as_str());
    let step = EditStep::record_command(&mut buffer, &edits).unwrap().step;
    let after = buffer.text();
    assert_eq!(after.matches("new").count(), 80);
    assert!(!after.contains("old"));

    let mut history = EditHistory::default();
    history.record(1, 2, step, 1_000);
    // A typing step right after the command still undoes on its own.
    let typed = EditStep::record(&mut buffer, &[support::edit((0, 0), (0, 0), "x")])
        .unwrap()
        .step;
    history.record(2, 3, typed, 1_010);
    history
        .replay(HistoryDirection::Undo, 3, &mut buffer)
        .unwrap()
        .unwrap();
    history.commit(HistoryDirection::Undo, 4);
    assert_eq!(buffer.text(), after);
    history
        .replay(HistoryDirection::Undo, 4, &mut buffer)
        .unwrap()
        .unwrap();
    history.commit(HistoryDirection::Undo, 5);
    assert_eq!(buffer.text(), text);

    let near = "a b a b a";
    let (near_edits, near_count) =
        replace_all(near, &matcher("a", true, false, false), "zz").unwrap();
    assert_eq!((near_edits.len(), near_count), (1, 3));
    assert_eq!(near_edits[0].text, "zz b zz b zz");
    let (none, zero) = replace_all(near, &matcher("q", true, false, false), "zz").unwrap();
    assert!(none.is_empty() && zero == 0);
}

#[test]
fn whole_file_find_on_the_largest_editor_buffer_stays_fast() {
    // The largest file the editor opens, with a match on every line: counting,
    // stepping and window projection must stay within the search bench budget.
    let line = "fn handle_request(context: &Context) -> Result<()> { needle }\n";
    let lines = soyeht_protocol::limits::EDITOR_HISTORY_BYTES / line.len() - 1;
    let text = line.repeat(lines);
    let needle = matcher("needle", true, false, false);
    let started = Instant::now();
    let outcome = find(
        &text,
        &FindRequest {
            matcher: &needle,
            from: position(u32::try_from(lines / 2).unwrap(), 0),
            direction: FindDirection::Previous,
            window: 0..2_000,
            visible_limit: 500,
        },
    );
    let elapsed = started.elapsed();
    assert_eq!(outcome.total, lines);
    assert_eq!(outcome.current.unwrap().index, lines / 2 - 1);
    assert_eq!(outcome.visible.len(), 500);
    assert!(
        elapsed < Duration::from_millis(1_000),
        "whole-file find took {elapsed:?}"
    );
}
