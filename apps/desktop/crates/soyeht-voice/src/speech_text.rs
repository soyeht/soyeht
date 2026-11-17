//! What of an agent's reply is spoken (ADR 0166 section 5, contract 20 section 3). The rules are in
//! `contracts/vectors/speech-text.json`, which the Apple clients pass too. The structure is the
//! shared Markdown parser's (ADR 0087): this module reads what the voice does with it.

use crate::held_back::held_back;
use crate::text::{is_letter, is_number, is_pictograph};
use soyeht_client::markdown::{Block, Run, parse};
use soyeht_protocol::client_policy::{
    VOICE_ADDRESS_OPENERS, VOICE_ADDRESS_PREFIXES, VOICE_SENTENCE_STOPS, VOICE_SENTENCE_WIDE_STOPS,
};

/// The reply as plain lines to read aloud, one per paragraph, heading or list item, joined by
/// newlines. Written text that is for the eyes is left out:
/// - code blocks and tables are not read, and neither is struck-out text or a rule;
/// - a link is read as its words, never its address; a bare web address is dropped, and a stop
///   that ended its sentence stays;
/// - HTML tags are dropped, and so are emoji and symbols a voice would read out as their names,
///   and a line with no letter or digit left with them;
/// - a task-list marker is dropped, and blanks are collapsed.
///
/// For a reply that is still arriving (`streaming`), what an unfinished construct would still
/// change is held back, and the text ends with a newline when its last line is finished: the next
/// block has begun, or the reply ends with a blank line.
pub fn speakable(markdown: &str, streaming: bool) -> String {
    let normalised = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let source = if streaming {
        held_back(&normalised)
    } else {
        &normalised
    };
    let blocks = parse(source, streaming);
    let mut lines = lines_of(&blocks);
    if streaming && !lines.is_empty() && last_line_is_finished(&blocks, source) {
        lines.push(String::new());
    }
    lines.join("\n")
}

fn lines_of(blocks: &[Block]) -> Vec<String> {
    blocks.iter().flat_map(lines_of_block).collect()
}

fn lines_of_block(block: &Block) -> Vec<String> {
    match block {
        Block::Paragraph(runs) | Block::Heading { runs, .. } => {
            clean(&spoken(runs)).into_iter().collect()
        }
        Block::Quote(blocks) => lines_of(blocks),
        Block::List { items, .. } => items.iter().flat_map(|item| lines_of(item)).collect(),
        Block::Code { .. } | Block::Table { .. } | Block::Rule => Vec::new(),
    }
}

fn spoken(runs: &[Run]) -> String {
    runs.iter()
        .filter(|run| !run.style.strike)
        .map(|run| run.text.as_str())
        .collect()
}

/// The last spoken line is complete when a block follows it, or the text ends in a blank line.
fn last_line_is_finished(blocks: &[Block], source: &str) -> bool {
    if matches!(
        blocks.last(),
        Some(Block::Code { .. } | Block::Table { .. } | Block::Rule)
    ) {
        return true;
    }
    let mut newlines = 0;
    for character in source.chars().rev() {
        match character {
            '\n' => newlines += 1,
            ' ' | '\t' => {}
            _ => break,
        }
    }
    newlines >= 2
}

const TASK_MARKERS: [&str; 3] = ["[ ] ", "[x] ", "[X] "];

fn clean(source: &str) -> Option<String> {
    let without_pictographs: String = source
        .chars()
        .filter(|&character| !is_pictograph(character))
        .collect();
    let mut text = without_tags(&without_pictographs);
    if let Some(marker) = TASK_MARKERS
        .iter()
        .find(|marker| text.starts_with(**marker))
    {
        text.drain(..marker.len());
    }
    let mut words: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        if !is_web_address(word) {
            words.push(word.to_owned());
            continue;
        }
        // The address is not read; the stop that ended the sentence it was in stays.
        let stop = &word[word.trim_end_matches(is_sentence_stop).len()..];
        if let Some(last) = words.last_mut() {
            last.push_str(stop);
        }
    }
    let line = words.join(" ");
    line.chars()
        .any(|character| is_letter(character) || is_number(character))
        .then_some(line)
}

fn is_sentence_stop(character: char) -> bool {
    VOICE_SENTENCE_STOPS.contains(character) || VOICE_SENTENCE_WIDE_STOPS.contains(character)
}

/// `text` without its HTML tags: `<`, then a letter, `/` or `!`, up to the next `>`.
fn without_tags(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let starts_tag = after
            .chars()
            .next()
            .is_some_and(|next| next.is_ascii_alphabetic() || next == '/' || next == '!');
        let close = starts_tag.then(|| after.find('>')).flatten();
        match close {
            Some(close) => {
                kept.push_str(&rest[..open]);
                rest = &after[close + 1..];
            }
            None => {
                kept.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    kept.push_str(rest);
    kept
}

fn is_web_address(word: &str) -> bool {
    let bare = word
        .trim_start_matches(|character| VOICE_ADDRESS_OPENERS.contains(character))
        .to_lowercase();
    VOICE_ADDRESS_PREFIXES
        .split('|')
        .any(|prefix| bare.starts_with(prefix))
}
