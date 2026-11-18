//! Rules of contract 20 that a vector file cannot carry: JSON has no NaN, and the line of a
//! number is a rule of the document that no vector case names.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_voice::{SentenceChunker, heard_text, speakable};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn a_line_with_a_digit_left_is_read() {
    assert_eq!(speakable("42", false), "42");
    assert_eq!(speakable("?! 42", false), "?! 42");
    assert_eq!(speakable("?! ...", false), "");
}

#[test]
fn a_share_that_is_not_a_number_is_none_of_the_sentence() {
    assert_eq!(
        heard_text(&["A.".to_owned()], Some("a b"), f64::NAN, None, 0.0),
        "A."
    );
}

#[test]
fn a_maximum_under_one_is_one_and_cannot_hang_the_run() {
    // The chunker runs on a thread of its own, so a loop that never ends fails the test at the
    // limit and does not hold the run.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut chunker = SentenceChunker::new(16, 0);
        let _ = sender.send(chunker.update("ab cd", true));
    });
    let emitted = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(emitted, ["a", "b", "c", "d"]);
}
