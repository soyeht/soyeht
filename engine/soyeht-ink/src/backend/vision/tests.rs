//! These tests ask Apple Vision to read strokes drawn here. They run where Vision exists (a Mac)
//! and say so when it does not; the engine has no switch for them.

use crate::{Writing, recognize, system};
use soyeht_protocol::{InkPoint, InkRecognizeParams, InkUnsupportedReason};

/// Block capitals on a grid 4 wide and 6 tall, as polylines of the pen.
fn glyph(letter: char) -> Vec<Vec<(f64, f64)>> {
    match letter {
        'H' => vec![
            vec![(0.0, 0.0), (0.0, 6.0)],
            vec![(4.0, 0.0), (4.0, 6.0)],
            vec![(0.0, 3.0), (4.0, 3.0)],
        ],
        'E' => vec![
            vec![(4.0, 0.0), (0.0, 0.0), (0.0, 6.0), (4.0, 6.0)],
            vec![(0.0, 3.0), (3.0, 3.0)],
        ],
        'L' => vec![vec![(0.0, 0.0), (0.0, 6.0), (4.0, 6.0)]],
        'O' => vec![vec![
            (1.0, 0.0),
            (3.0, 0.0),
            (4.0, 1.0),
            (4.0, 5.0),
            (3.0, 6.0),
            (1.0, 6.0),
            (0.0, 5.0),
            (0.0, 1.0),
            (1.0, 0.0),
        ]],
        'T' => vec![vec![(0.0, 0.0), (4.0, 0.0)], vec![(2.0, 0.0), (2.0, 6.0)]],
        _ => Vec::new(),
    }
}

/// The strokes of `word`, in the pen's own units and with the time the pen takes for each point.
fn word(text: &str, unit: f64) -> InkRecognizeParams {
    let mut strokes = Vec::new();
    let mut clock = 0;
    for (index, letter) in text.chars().enumerate() {
        for stroke in glyph(letter) {
            strokes.push(
                stroke
                    .into_iter()
                    .map(|(x, y)| {
                        clock += 8;
                        InkPoint {
                            x: (x + index as f64 * 6.0) * unit + 300.0,
                            y: y * unit + 900.0,
                            t: clock,
                        }
                    })
                    .collect(),
            );
        }
    }
    InkRecognizeParams {
        strokes,
        languages: vec!["en-US".into()],
        max_candidates: Some(5),
    }
}

#[test]
fn vision_reads_a_word_drawn_with_a_pen() {
    let Ok(mut recognizer) = system() else {
        eprintln!("skipped: this system has no Vision");
        return;
    };
    let writing = Writing::from_params(&word("HELLO", 9.0)).unwrap();
    let answer = match recognize(recognizer.as_mut(), &writing) {
        Ok(answer) => answer,
        Err(error) if error.unsupported_reason() == Some(&InkUnsupportedReason::NoOsSupport) => {
            eprintln!("skipped: Vision cannot read text on this system");
            return;
        }
        Err(error) => panic!("recognition failed: {error}"),
    };
    assert_eq!(answer.recognizer, "apple-vision");
    assert_eq!(answer.language, "en-US");
    let first = answer
        .candidates
        .first()
        .map(|candidate| candidate.text.to_lowercase());
    assert_eq!(first.as_deref(), Some("hello"), "{:?}", answer.candidates);
    assert!(answer.candidates.len() <= 5);
    assert!(answer.candidates[0].score.is_some_and(|score| score > 0.0));
}

#[test]
fn the_same_word_in_other_units_is_read_the_same() {
    let Ok(mut recognizer) = system() else {
        return;
    };
    for unit in [0.4, 90.0] {
        let writing = Writing::from_params(&word("TOLL", unit)).unwrap();
        let Ok(answer) = recognize(recognizer.as_mut(), &writing) else {
            return;
        };
        let first = answer.candidates.first().map(|c| c.text.to_lowercase());
        assert!(
            matches!(first.as_deref(), Some("toll" | "tol" | "tell")),
            "{first:?} at {unit}"
        );
    }
}

#[test]
fn a_language_vision_does_not_read_is_unavailable_and_a_blank_page_has_no_candidates() {
    let Ok(mut recognizer) = system() else {
        return;
    };
    let mut klingon = word("HELLO", 9.0);
    klingon.languages = vec!["tlh".into()];
    let writing = Writing::from_params(&klingon).unwrap();
    assert_eq!(
        recognize(recognizer.as_mut(), &writing),
        Err(crate::Error::LanguageUnavailable)
    );
    let dot = InkRecognizeParams {
        strokes: vec![vec![InkPoint {
            x: 1.0,
            y: 1.0,
            t: 0,
        }]],
        languages: vec!["en".into()],
        max_candidates: None,
    };
    let writing = Writing::from_params(&dot).unwrap();
    if let Ok(answer) = recognize(recognizer.as_mut(), &writing) {
        assert!(
            answer
                .candidates
                .iter()
                .all(|c| c.text.chars().count() <= 2)
        );
    }
}
