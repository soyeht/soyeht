//! Letters to phonemes by rules of our own, for a TTS that gives only audio.
//!
//! This is a rough track, and it is meant to be: the mouth only needs the lip shape of each
//! sound, and the aligner places the result on the audio that actually plays. Portuguese (pt-BR)
//! follows the lab's `phonemesFromWords` (Apache-2.0, the same authors), with the accented vowel
//! marked as the stressed one. English is a smaller table of the same kind (digraphs, the
//! silent final e, the common vowel pairs). No external phonemizer is used: the usual one is GPL
//! (ADR 0166, licenses).

use crate::phoneme::{STRESS, is_pause, is_semi, vowel};

/// The language whose letter rules read the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    PortugueseBrazil,
    English,
}

/// One piece of a text: a word, or a pause mark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    Word(String),
    Pause(char),
}

/// Split a text into words and pause marks. Everything that is neither a letter nor a pause mark
/// separates words and is dropped.
pub(crate) fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c.is_alphabetic() {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            out.push(Token::Word(std::mem::take(&mut word)));
        }
        if is_pause(c) && c != '-' {
            out.push(Token::Pause(c));
        }
    }
    if !word.is_empty() {
        out.push(Token::Word(word));
    }
    out
}

/// A phoneme and whether the letter that gave it carries the written accent.
type Seq = Vec<(char, bool)>;

/// The phonemes of one word, with the stress mark `ˈ` placed before the stressed vowel: the
/// accented one when the word has an accent, else the first.
pub(crate) fn phonemize_word(word: &str, language: Language) -> Vec<char> {
    let letters: Vec<char> = word.to_lowercase().chars().collect();
    let seq = match language {
        Language::PortugueseBrazil => portuguese(&letters),
        Language::English => english(&letters),
    };
    let is_vowel = |p: char| vowel(p).is_some() && !is_semi(p);
    let stressed = seq
        .iter()
        .position(|(p, accent)| *accent && is_vowel(*p))
        .or_else(|| seq.iter().position(|(p, _)| is_vowel(*p)));
    let mut out = Vec::with_capacity(seq.len() + 1);
    for (i, (p, _)) in seq.iter().enumerate() {
        if Some(i) == stressed {
            out.push(STRESS);
        }
        out.push(*p);
    }
    out
}

/// Two letters that make one sound (or a fixed pair of sounds).
type Digraphs = &'static [(&'static str, &'static str)];

const PORTUGUESE_DIGRAPHS: Digraphs = &[
    ("ch", "ʃ"),
    ("lh", "ʎ"),
    ("nh", "ɲ"),
    ("rr", "x"),
    ("ss", "s"),
    ("qu", "k"),
];

const ENGLISH_DIGRAPHS: Digraphs = &[
    ("th", "θ"),
    ("sh", "ʃ"),
    ("ch", "ʧ"),
    ("ph", "f"),
    ("ck", "k"),
    ("qu", "kw"),
    ("wh", "w"),
    ("ng", "n"),
    ("ee", "i"),
    ("ea", "i"),
    ("oo", "u"),
    ("oa", "o"),
    ("ou", "ɑʊ"),
    ("ow", "ɑʊ"),
    ("ai", "eɪ"),
    ("ay", "eɪ"),
    ("oi", "ɔɪ"),
    ("oy", "ɔɪ"),
];

fn digraph(table: Digraphs, a: char, b: Option<char>) -> Option<&'static str> {
    let b = b?;
    table
        .iter()
        .find(|(letters, _)| letters.chars().eq([a, b]))
        .map(|(_, sound)| *sound)
}

fn portuguese_fold(c: char) -> Option<char> {
    Some(match c {
        'á' | 'à' | 'â' => 'a',
        'ã' => 'ɐ',
        'é' => 'ɛ',
        'ê' => 'e',
        'í' => 'i',
        'ó' => 'ɔ',
        'ô' | 'õ' => 'o',
        'ú' | 'ü' => 'u',
        _ => return None,
    })
}

/// Where the walk over a word is: the letter and the two after it.
struct At {
    i: usize,
    c: char,
    next: Option<char>,
    after: Option<char>,
}

/// Walk over the letters of a word: a digraph of the table makes its sounds, and any other letter
/// is handed to `one`, which pushes its sounds and returns how many further letters it used.
fn walk(chars: &[char], digraphs: Digraphs, mut one: impl FnMut(&mut Seq, At) -> usize) -> Seq {
    let mut seq: Seq = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let next = chars.get(i + 1).copied();
        if let Some(sound) = digraph(digraphs, chars[i], next) {
            seq.extend(sound.chars().map(|p| (p, false)));
            i += 2;
            continue;
        }
        let at = At {
            i,
            c: chars[i],
            next,
            after: chars.get(i + 2).copied(),
        };
        i += 1 + one(&mut seq, at);
    }
    seq
}

fn portuguese(chars: &[char]) -> Seq {
    let last = chars.len().saturating_sub(1);
    let front = |c: Option<char>| matches!(c, Some('e' | 'é' | 'ê' | 'i' | 'í'));
    let vowel_letter = |c: Option<char>| c.is_some_and(|c| "aeiouáéêíóôúãõ".contains(c));
    walk(
        chars,
        PORTUGUESE_DIGRAPHS,
        |seq, At { i, c, next, after }| {
            match (c, next) {
                ('g', Some('u')) if front(after) => {
                    seq.push(('g', false));
                    return 1;
                }
                ('ç', _) => seq.push(('s', false)),
                ('x', _) => seq.push(('ʃ', false)),
                ('h', _) => {}
                ('c', _) => seq.push((if front(next) { 's' } else { 'k' }, false)),
                ('g', _) => seq.push((if front(next) { 'ʒ' } else { 'g' }, false)),
                ('j', _) => seq.push(('ʒ', false)),
                ('m' | 'n', _) => {
                    // A nasal before a consonant or at the end nasalises the vowel before it and has
                    // no lips of its own, except a final m, which closes them.
                    if vowel_letter(next) {
                        seq.push((c, false));
                    } else if c == 'm' && i == last && !seq.is_empty() {
                        seq.push(('m', false));
                    }
                }
                _ if portuguese_fold(c).is_some() => seq.push((
                    portuguese_fold(c).unwrap_or(c),
                    matches!(c, 'á' | 'â' | 'é' | 'ê' | 'í' | 'ó' | 'ô' | 'ú'),
                )),
                ('a' | 'e' | 'i' | 'o' | 'u', _) => {
                    let word_end = i == last || (i + 2 == chars.len() && next == Some('s'));
                    let p = match (word_end, c) {
                        (true, 'o') => 'ʊ',
                        (true, 'e') => 'ɪ',
                        (true, 'a') => 'ɐ',
                        _ => c,
                    };
                    seq.push((p, false));
                }
                _ if "bpmfvtdnlrszkg".contains(c) => {
                    seq.push((if c == 'r' { 'ɾ' } else { c }, false))
                }
                _ => {}
            }
            0
        },
    )
}

fn english(chars: &[char]) -> Seq {
    walk(chars, ENGLISH_DIGRAPHS, |seq, At { i, c, next, .. }| {
        let front = matches!(next, Some('e' | 'i' | 'y'));
        let silent_final_e =
            c == 'e' && i + 1 == chars.len() && chars.len() > 3 && !"aeiouy".contains(chars[i - 1]);
        match c {
            _ if silent_final_e => {}
            'a' => seq.push(('æ', false)),
            'e' => seq.push(('ɛ', false)),
            'i' => seq.push(('ɪ', false)),
            'o' => seq.push(('ɑ', false)),
            'u' => seq.push(('ɐ', false)),
            'y' if i == 0 => seq.push(('j', false)),
            'y' => seq.push(('i', false)),
            'c' => seq.push((if front { 's' } else { 'k' }, false)),
            'g' => seq.push((if front { 'ʤ' } else { 'g' }, false)),
            'j' => seq.push(('ʤ', false)),
            'x' => {
                seq.push(('k', false));
                seq.push(('s', false));
            }
            'r' => seq.push(('ɹ', false)),
            _ if c.is_ascii_lowercase() => seq.push((c, false)),
            _ => {}
        }
        0
    })
}
