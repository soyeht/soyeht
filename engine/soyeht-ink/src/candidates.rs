//! What a recognizer says, made into what the contract answers: the languages it reads that
//! were asked for, the text of several lines put in reading order, and a list of clean,
//! distinct, bounded candidates.

use crate::error::{Error, Result};
use soyeht_protocol::{InkCandidate, limits};

/// One candidate as a recognizer gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub text: String,
    /// How sure the recognizer is, when it says (0 to 1).
    pub score: Option<f64>,
}

/// A piece of text a recognizer found at a place of the image, whose origin is the top left
/// corner and whose unit is the image's height.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub text: String,
    pub score: Option<f64>,
    pub left: f64,
    pub top: f64,
    pub height: f64,
}

/// The tags of `available` that read what `requested` asks, in the order asked and with none
/// twice. A tag matches by its whole self (without regard to case) or, failing that, by its
/// language: `pt-PT` is read by a recognizer that has `pt-BR`.
pub fn languages(requested: &[String], available: &[String]) -> Result<Vec<String>> {
    let mut found: Vec<String> = Vec::new();
    for tag in requested {
        let exact = available
            .iter()
            .find(|tag_of| tag_of.eq_ignore_ascii_case(tag));
        let same_language = || {
            let language = primary(tag);
            available
                .iter()
                .find(|tag_of| primary(tag_of).eq_ignore_ascii_case(language))
        };
        if let Some(read) = exact.or_else(same_language)
            && !found.contains(read)
        {
            found.push(read.clone());
        }
    }
    if found.is_empty() {
        Err(Error::LanguageUnavailable)
    } else {
        Ok(found)
    }
}

fn primary(tag: &str) -> &str {
    tag.split(['-', '_']).next().unwrap_or(tag)
}

/// The pieces as one text: lines from the top down, the pieces of a line from left to right.
/// A piece belongs to the line of the first piece above it that it overlaps by half its height.
pub fn reading_order(mut pieces: Vec<Piece>) -> Option<Candidate> {
    pieces.retain(|piece| !piece.text.trim().is_empty());
    pieces.sort_by(|a, b| a.top.total_cmp(&b.top));
    let mut lines: Vec<(f64, f64, Vec<&Piece>)> = Vec::new();
    for piece in &pieces {
        let middle = piece.top + piece.height / 2.0;
        match lines.iter_mut().find(|(top, bottom, _)| {
            let overlap = bottom.min(piece.top + piece.height) - top.max(piece.top);
            overlap >= piece.height / 2.0 || (*top..=*bottom).contains(&middle)
        }) {
            Some((top, bottom, members)) => {
                *top = top.min(piece.top);
                *bottom = bottom.max(piece.top + piece.height);
                members.push(piece);
            }
            None => lines.push((piece.top, piece.top + piece.height, vec![piece])),
        }
    }
    if lines.is_empty() {
        return None;
    }
    let mut text = String::new();
    let (mut scores, mut count) = (0.0, 0usize);
    for (index, (_, _, mut members)) in lines.into_iter().enumerate() {
        members.sort_by(|a, b| a.left.total_cmp(&b.left));
        if index > 0 {
            text.push('\n');
        }
        for (position, piece) in members.iter().enumerate() {
            if position > 0 {
                text.push(' ');
            }
            text.push_str(piece.text.trim());
            if let Some(score) = piece.score {
                scores += score;
                count += 1;
            }
        }
    }
    Some(Candidate {
        text,
        score: (count > 0).then(|| scores / count as f64),
    })
}

/// The answer's candidates: trimmed, not empty, not twice, each cut to the contract's size,
/// with a score only when it is a number between 0 and 1, and no more than `most`.
pub fn tidy(raw: Vec<Candidate>, most: usize) -> Vec<InkCandidate> {
    let mut kept: Vec<InkCandidate> = Vec::new();
    for candidate in raw {
        let text = cut(candidate.text.trim(), limits::INK_CANDIDATE_BYTES);
        if text.is_empty() || kept.iter().any(|known| known.text == text) {
            continue;
        }
        kept.push(InkCandidate {
            text: text.to_owned(),
            score: candidate
                .score
                .filter(|score| score.is_finite())
                .map(|score| score.clamp(0.0, 1.0)),
        });
        if kept.len() == most {
            break;
        }
    }
    kept
}

/// `text` cut to at most `bytes` bytes, at a character.
fn cut(text: &str, bytes: usize) -> &str {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|tag| (*tag).to_owned()).collect()
    }

    fn piece(text: &str, left: f64, top: f64) -> Piece {
        Piece {
            text: text.into(),
            score: Some(0.5),
            left,
            top,
            height: 0.1,
        }
    }

    #[test]
    fn languages_match_by_tag_then_by_language_in_the_order_asked() {
        let available = tags(&["en-US", "pt-BR", "zh-Hans"]);
        assert_eq!(
            languages(
                &tags(&["PT-br", "en-GB", "pt-PT", "zh-CN", "en-US"]),
                &available
            )
            .unwrap(),
            ["pt-BR", "en-US", "zh-Hans"]
        );
    }

    #[test]
    fn languages_nobody_reads_are_unavailable() {
        assert_eq!(
            languages(&tags(&["sv", "tlh"]), &tags(&["en-US"])),
            Err(Error::LanguageUnavailable)
        );
        assert_eq!(
            languages(&tags(&["en"]), &[]),
            Err(Error::LanguageUnavailable)
        );
    }

    #[test]
    fn pieces_are_read_top_down_and_left_to_right() {
        let found = reading_order(vec![
            piece("world", 0.5, 0.52),
            piece("second", 0.0, 0.5),
            piece("hello", 0.5, 0.0),
            piece("first", 0.0, 0.02),
        ])
        .unwrap();
        assert_eq!(found.text, "first hello\nsecond world");
        assert_eq!(found.score, Some(0.5));
    }

    #[test]
    fn a_blank_piece_is_not_text_and_no_piece_is_no_candidate() {
        assert_eq!(reading_order(vec![piece("  ", 0.0, 0.0)]), None);
        assert_eq!(reading_order(vec![]), None);
        let mut no_score = piece("x", 0.0, 0.0);
        no_score.score = None;
        assert_eq!(reading_order(vec![no_score]).unwrap().score, None);
    }

    #[test]
    fn candidates_are_trimmed_distinct_bounded_and_scored_within_range() {
        let raw = ["  ola ", "ola", "", "   ", "ala", "ole", "olá"]
            .into_iter()
            .enumerate()
            .map(|(index, text)| Candidate {
                text: text.into(),
                score: Some([0.9, 0.8, 0.7, 0.6, 1.5, f64::NAN, -1.0][index]),
            })
            .collect();
        let kept = tidy(raw, 3);
        assert_eq!(
            kept.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
            ["ola", "ala", "ole"]
        );
        assert_eq!(kept[0].score, Some(0.9));
        assert_eq!(kept[1].score, Some(1.0));
        assert_eq!(kept[2].score, None);
    }

    #[test]
    fn a_long_candidate_is_cut_at_a_character_and_a_missing_one_leaves_an_empty_list() {
        let long = "ç".repeat(limits::INK_CANDIDATE_BYTES);
        let kept = tidy(
            vec![Candidate {
                text: long,
                score: None,
            }],
            5,
        );
        assert!(kept[0].text.len() <= limits::INK_CANDIDATE_BYTES);
        assert!(kept[0].text.chars().all(|c| c == 'ç'));
        assert!(tidy(vec![], 5).is_empty());
    }
}
