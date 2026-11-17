//! Rules of contract 20 that a vector file cannot carry: JSON has no NaN, and the line of a
//! number is a rule of the document that no vector case names.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_voice::{heard_text, speakable};

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
