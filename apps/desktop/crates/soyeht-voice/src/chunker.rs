//! Cuts the speakable text of a reply that is still arriving into the sentences a voice speaks,
//! the first as early as it is whole (contract 20, section 3). Lengths count Unicode scalar values
//! and a blank is a character with the Unicode `White_Space` property; the stops, closers,
//! abbreviations and limits are `x-clientPolicy.voice` (`sentence...`).

use crate::text::trim_blanks;
use soyeht_protocol::client_policy::{
    VOICE_SENTENCE_ABBREVIATIONS, VOICE_SENTENCE_CLOSERS, VOICE_SENTENCE_MAX_CHARS,
    VOICE_SENTENCE_MIN_CHARS, VOICE_SENTENCE_STOPS, VOICE_SENTENCE_WIDE_STOPS,
};

fn chars_of(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn is_stop(character: char) -> bool {
    VOICE_SENTENCE_STOPS.contains(character)
}

fn is_wide_stop(character: char) -> bool {
    VOICE_SENTENCE_WIDE_STOPS.contains(character)
}

/// The sentences of one line that can be spoken now.
struct Pieces {
    /// What to speak, in order.
    spoken: Vec<String>,
    /// How many characters of the line the pieces cover (all of it when the line is finished).
    covered: usize,
    /// Where each piece ends in the line.
    ends: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct SentenceChunker {
    minimum: usize,
    maximum: usize,
    consumed: usize,
    previous: Vec<char>,
    boundaries: Vec<usize>,
}

impl Default for SentenceChunker {
    fn default() -> Self {
        Self::new(
            chars_of(VOICE_SENTENCE_MIN_CHARS),
            chars_of(VOICE_SENTENCE_MAX_CHARS),
        )
    }
}

impl SentenceChunker {
    /// A sentence shorter than `minimum` waits for the next one of its line; one longer than
    /// `maximum` is cut.
    pub fn new(minimum: usize, maximum: usize) -> Self {
        Self {
            minimum,
            maximum,
            consumed: 0,
            previous: Vec::new(),
            boundaries: Vec::new(),
        }
    }

    /// The sentences that became whole with this text: `spoken` is all the speakable text so far,
    /// and `is_final` says the reply is over, so what is left is spoken as it is.
    pub fn update(&mut self, spoken: &str, is_final: bool) -> Vec<String> {
        let text: Vec<char> = spoken.chars().collect();
        self.resynchronize(&text);
        let mut emitted = Vec::new();
        let mut line_start = self.consumed.min(text.len());
        while line_start <= text.len() {
            let newline = text[line_start..]
                .iter()
                .position(|&character| character == '\n')
                .map(|offset| line_start + offset);
            let closed = newline.is_some() || is_final;
            let end = newline.unwrap_or(text.len());
            let line = &text[line_start..end];
            let pieces = self.pieces(line, closed);
            emitted.extend(pieces.spoken);
            self.boundaries
                .extend(pieces.ends.iter().map(|end| line_start + end));
            let Some(newline) = newline else {
                self.consumed = line_start + if closed { line.len() } else { pieces.covered };
                break;
            };
            line_start = newline + 1;
            self.consumed = line_start;
            self.boundaries.push(line_start);
        }
        emitted
    }

    /// A text that no longer begins with what was cut goes back to the last sentence end both share.
    fn resynchronize(&mut self, text: &[char]) {
        let shared = self
            .previous
            .iter()
            .zip(text)
            .take_while(|(old, new)| old == new)
            .count();
        self.previous = text.to_vec();
        if shared >= self.consumed {
            return;
        }
        self.consumed = self
            .boundaries
            .iter()
            .rev()
            .copied()
            .find(|&boundary| boundary <= shared)
            .unwrap_or(0);
        let consumed = self.consumed;
        self.boundaries.retain(|&boundary| boundary <= consumed);
    }

    fn pieces(&self, line: &[char], closed: bool) -> Pieces {
        let mut units: Vec<(String, usize)> = Vec::new();
        let mut start = 0;
        let mut index = 0;
        while index < line.len() {
            match sentence_end(line, index, closed) {
                Some(end) => {
                    units.push((text_of(&line[start..end]), end));
                    start = end;
                    index = end;
                }
                None => index += 1,
            }
        }
        let tail = start;
        if closed {
            units.push((text_of(&line[tail..]), line.len()));
        }
        units.retain(|(text, _)| !text.is_empty());

        let mut pieces = Pieces {
            spoken: Vec::new(),
            covered: 0,
            ends: Vec::new(),
        };
        let mut pending = String::new();
        let last = units.len().saturating_sub(1);
        for (position, (text, end)) in units.into_iter().enumerate() {
            pending = join(&pending, &text);
            if pending.chars().count() < self.minimum && !(closed && position == last) {
                continue;
            }
            let scalars: Vec<char> = pending.chars().collect();
            pieces
                .spoken
                .extend(cut(&scalars, self.maximum).into_iter().map(|part| part.0));
            pending.clear();
            pieces.covered = end;
            pieces.ends.push(end);
        }
        if closed {
            pieces.covered = line.len();
            return pieces;
        }
        // An unfinished tail that has outgrown the limit is spoken in parts; the rest waits.
        if pending.is_empty() {
            let parts = cut(&line[tail..], self.maximum);
            for (text, end) in parts.iter().take(parts.len().saturating_sub(1)) {
                pieces.spoken.push(text.clone());
                pieces.covered = tail + end;
                pieces.ends.push(pieces.covered);
            }
        }
        pieces
    }
}

/// The two pieces as one sentence: a blank between them, except after a stop of a script that
/// writes no blanks.
fn join(first: &str, second: &str) -> String {
    match first.chars().last() {
        None => second.to_owned(),
        Some(last) if is_wide_stop(last) => format!("{first}{second}"),
        Some(_) => format!("{first} {second}"),
    }
}

/// Where the sentence that stops at `index` ends in `line` (after its stops and closers), or none.
fn sentence_end(line: &[char], index: usize, closed: bool) -> Option<usize> {
    let scalar = line[index];
    if !is_stop(scalar) && !is_wide_stop(scalar) {
        return None;
    }
    let mut end = index + 1;
    while end < line.len() && (is_stop(line[end]) || is_wide_stop(line[end])) {
        end += 1;
    }
    while end < line.len() && VOICE_SENTENCE_CLOSERS.contains(line[end]) {
        end += 1;
    }
    if is_wide_stop(scalar) {
        return Some(end);
    }
    if end < line.len() {
        if !line[end].is_whitespace() {
            return None;
        }
    } else if !closed {
        return None;
    }
    if scalar != '.' {
        return Some(end);
    }
    if end == index + 1 && is_abbreviation(&line[..=index]) {
        return None;
    }
    // A full stop followed by a word in lowercase does not end the sentence; one with nothing
    // after it yet may still be followed by one.
    match line[end..]
        .iter()
        .find(|character| !character.is_whitespace())
    {
        None => closed.then_some(end),
        Some(next) => (!next.is_lowercase()).then_some(end),
    }
}

fn is_abbreviation(up_to_stop: &[char]) -> bool {
    let word: String = up_to_stop
        .iter()
        .rev()
        .take_while(|character| !character.is_whitespace())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let word = word.to_lowercase();
    VOICE_SENTENCE_ABBREVIATIONS
        .split('|')
        .any(|abbreviation| abbreviation == word)
}

fn text_of(scalars: &[char]) -> String {
    let text: String = scalars.iter().collect();
    trim_blanks(&text).to_owned()
}

/// `scalars` as one piece, or several when it is longer than `maximum`; each part with where in
/// `scalars` it stops. A part with nothing to say is not one.
fn cut(scalars: &[char], maximum: usize) -> Vec<(String, usize)> {
    let mut parts = Vec::new();
    let mut from = 0;
    while scalars.len() - from > maximum {
        let window = from..from + maximum;
        let clause = window.clone().rev().find(|&index| {
            matches!(scalars[index], ',' | ';' | ':')
                && scalars
                    .get(index + 1)
                    .is_some_and(|next| next.is_whitespace())
        });
        let at = clause
            .map(|index| index + 1)
            .or_else(|| {
                window
                    .clone()
                    .rev()
                    .find(|&index| scalars[index].is_whitespace())
            })
            .unwrap_or(window.end);
        parts.push((text_of(&scalars[from..at]), at));
        from = at;
        while from < scalars.len() && scalars[from].is_whitespace() {
            from += 1;
        }
    }
    let last = text_of(&scalars[from..]);
    if !last.is_empty() {
        parts.push((last, scalars.len()));
    }
    parts.retain(|(text, _)| !text.is_empty());
    parts
}
