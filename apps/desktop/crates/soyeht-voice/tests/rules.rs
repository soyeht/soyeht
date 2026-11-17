//! Rules of contract 20, sections 3 and 4, that the sentences of the document state and no case of
//! the vector files pins yet. They are the document's own words, not the Apple clients' code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_voice::{ReplyFeed, SentenceChunker, WordTiming, heard_text, speakable};

#[test]
fn a_line_with_a_digit_left_is_read() {
    assert_eq!(speakable("42", false), "42");
    assert_eq!(speakable("?! 42", false), "?! 42");
    assert_eq!(speakable("?! ...", false), "");
}

#[test]
fn a_tag_is_an_angle_bracket_then_a_letter_a_slash_or_a_bang_up_to_the_next_one() {
    assert_eq!(speakable("a <!-- note --> b", false), "a b");
    assert_eq!(speakable("a < b > c", false), "a < b > c");
    assert_eq!(speakable("a <b c", false), "a <b c");
}

#[test]
fn a_full_stop_after_an_abbreviation_does_not_end_a_sentence_of_any_length() {
    let mut chunker = SentenceChunker::default();
    let spoken = "Please call my friend Dr. Smith about it today. Thanks";
    assert_eq!(
        chunker.update(spoken, false),
        ["Please call my friend Dr. Smith about it today."]
    );
}

#[test]
fn a_reply_that_is_still_arriving_holds_back_what_is_unfinished() {
    let mut feed = ReplyFeed::default();
    let markdown = "Hello there, my friend. Please look at [this long link. Then";
    let update = feed.update("a", markdown, false);
    assert_eq!(update.sentences, ["Hello there, my friend."]);
}

#[test]
fn a_feed_remembers_only_the_last_identities() {
    let mut feed = ReplyFeed::default();
    for id in 0..65 {
        feed.update(&id.to_string(), "Hello there, my friend. How", false);
    }
    feed.cancel();
    assert!(
        !feed
            .update("1", "Hello there, my friend. How", false)
            .accepted
    );
    assert!(
        !feed
            .update("64", "Hello there, my friend. How", false)
            .accepted
    );
    assert!(
        feed.update("0", "Hello there, my friend. How", false)
            .accepted
    );
}

#[test]
fn only_the_words_a_share_covers_whole_are_heard() {
    let heard = heard_text(&[], Some("a b c"), 0.5, None, 0.0);
    assert_eq!(heard, "a");
}

#[test]
fn the_last_word_is_heard_only_when_all_of_the_sentence_was() {
    let word = |text: &str, start| WordTiming {
        text: text.to_owned(),
        start,
    };
    let timings = [word("one", 0.0), word("two", 0.5), word("three.", 1.0)];
    let heard = heard_text(&[], Some("one two three."), 0.95, Some(&timings), 5.0);
    assert_eq!(heard, "one two");
}
