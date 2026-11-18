//! What the person heard of the reply they interrupted (contract 20, section 3): the sentences
//! played to their end, and the part of the one playing that its position in the audio covers.

use crate::text::{is_blankless, trim_blanks};

/// A word of the sentence playing and the second of the audio at which the voice began it.
#[derive(Debug, Clone, PartialEq)]
pub struct WordTiming {
    pub text: String,
    pub start: f64,
}

/// The words of a reply the person heard before they talked over it. `played` are the sentences
/// played to their end; `current` is the one that was playing, of which `fraction` (0 to 1) had
/// been heard. With the voice's own word `timings`, a word is heard once the next one has started
/// (the last one when all of the sentence was heard) at `position` seconds. Without them, the words
/// that the fraction covers are kept, a word being a run of non-blank characters and each
/// character of a script with no blanks a word of its own; the text is cut after the last word
/// that is whole.
pub fn heard_text(
    played: &[String],
    current: Option<&str>,
    fraction: f64,
    timings: Option<&[WordTiming]>,
    position: f64,
) -> String {
    let share = if fraction.is_nan() {
        0.0
    } else {
        fraction.clamp(0.0, 1.0)
    };
    let partial = match timings.filter(|words| !words.is_empty()) {
        Some(words) => {
            let heard = (0..words.len())
                .filter(|&index| match words.get(index + 1) {
                    Some(next) => next.start <= position,
                    None => share >= 1.0,
                })
                .count();
            words[..heard]
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        }
        None => current.map_or_else(String::new, |text| prefix_of(text, share)),
    };
    played
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(partial.as_str()))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn prefix_of(text: &str, share: f64) -> String {
    let scalars: Vec<char> = text.chars().collect();
    let mut ends = Vec::new();
    let mut index = 0;
    while index < scalars.len() {
        if scalars[index].is_whitespace() {
            index += 1;
        } else if is_blankless(scalars[index]) {
            index += 1;
            ends.push(index);
        } else {
            while index < scalars.len()
                && !scalars[index].is_whitespace()
                && !is_blankless(scalars[index])
            {
                index += 1;
            }
            ends.push(index);
        }
    }
    // The share is within 0 to 1, so the count is within the number of words.
    let count = (share * ends.len() as f64).floor() as usize;
    let Some(&end) = count.checked_sub(1).and_then(|last| ends.get(last)) else {
        return String::new();
    };
    let kept: String = scalars[..end].iter().collect();
    trim_blanks(&kept).to_owned()
}
