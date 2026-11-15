//! The audio-to-mouth aligner (tier 2 of ADR 0166, section 6): the text of a reply and the
//! envelope of the audio that actually plays become a timed phoneme track, for a TTS that gives
//! no timings.
//!
//! 1. The text becomes phonemes by our own letter rules (`letters`).
//! 2. The envelope gives the voiced span of the clip and the silences inside it (`voiced`).
//! 3. Each word gets its share of the voiced time in proportion to its sounds; the boundaries the
//!    text predicts are matched to the silences the audio shows, and the words are stretched to
//!    fit between them (`warp`). Pause marks take the span of the silence they met.
//!
//! This uses the voiced span and the silences only. It does not listen to the vowels: that
//! would be a trained model, which the ADR leaves out of the launch.

pub(crate) mod letters;
mod voiced;
mod warp;

pub use letters::Language;

use crate::clip::WordStart;
use crate::envelope::Envelope;
use crate::phoneme::{Phoneme, STRESS, is_pause, vowel};
use letters::{Token, phonemize_word, tokens};
use voiced::{Valley, find_voiced};
use warp::{Boundary, Warp, match_boundaries};

/// The most sounds a second of speech holds. The lab's real clips have 13 to 14 a second, fast
/// speech about twice that; a text with more than this in the audio it is placed on is not that
/// audio's text.
const MAX_SOUNDS_PER_SECOND: f64 = 30.0;

/// The share of a word's time that a consonant takes beside a vowel's 1.
const CONSONANT_WEIGHT: f64 = 0.55;
/// Tolerance for comparing a warped position with a silence's position (seconds).
const EPSILON: f64 = 1e-9;

/// The result of aligning a text to a clip.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    /// The phonemes on the clip's time, ready for `Clip::new`. Empty when the audio has no voice
    /// or the text no sound.
    pub phonemes: Vec<Phoneme>,
    /// The voiced span of the audio, in seconds.
    pub span: Option<(f64, f64)>,
    /// How many boundaries between words were paired with a silence of the audio, out of
    /// `boundaries`.
    pub matched: usize,
    pub boundaries: usize,
    /// When each word of the text starts, for the accents of the acting layer.
    pub words: Vec<WordStart>,
    /// False when the text has more sounds than speech can hold in this audio (more than
    /// `MAX_SOUNDS_PER_SECOND`): the text does not belong to the audio, and the phonemes are left
    /// empty, so the runtime follows the level of the voice. The timings a TTS gives are never
    /// judged this way: this compares a text with audio, which only the aligner does.
    pub fits: bool,
}

struct Word {
    text: String,
    phonemes: Vec<char>,
    weight: f64,
    pause: Option<char>,
}

fn weight(p: char) -> f64 {
    if p == STRESS {
        0.0
    } else if vowel(p).is_some() {
        1.0
    } else {
        CONSONANT_WEIGHT
    }
}

fn words(text: &str, language: Language) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    for token in tokens(text) {
        match token {
            Token::Word(w) => {
                let phonemes = phonemize_word(&w, language);
                let weight = phonemes.iter().map(|p| weight(*p)).sum();
                if weight > 0.0 {
                    out.push(Word {
                        text: w,
                        phonemes,
                        weight,
                        pause: None,
                    });
                }
            }
            Token::Pause(p) => {
                if let Some(last) = out.last_mut().filter(|w| w.pause.is_none()) {
                    last.pause = Some(p);
                }
            }
        }
    }
    out
}

/// The silences of the voiced span, with where each one sits in voiced time (the time with the
/// silences taken out) and how much silence lies before it.
struct Valleys {
    all: Vec<Valley>,
    positions: Vec<f64>,
    /// `before[k]` is the total duration of the first `k` silences.
    before: Vec<f64>,
}

impl Valleys {
    fn new(span_start: f64, valleys: &[Valley]) -> Self {
        let mut positions = Vec::with_capacity(valleys.len());
        let mut before = vec![0.0];
        for v in valleys {
            let gone = before.last().copied().unwrap_or(0.0);
            positions.push((v.start - span_start) - gone);
            before.push(gone + v.duration());
        }
        Self {
            all: valleys.to_vec(),
            positions,
            before,
        }
    }

    /// Voiced time to clip time: the silences before the point are put back. `after` says whether
    /// a silence that starts exactly at the point is before it (the start of the next word) or
    /// after it (the end of the word).
    fn to_clip(&self, span_start: f64, y: f64, after: bool) -> f64 {
        let count = if after {
            self.positions.partition_point(|pos| *pos <= y + EPSILON)
        } else {
            self.positions.partition_point(|pos| *pos < y - EPSILON)
        };
        span_start + y + self.before[count]
    }
}

fn nothing() -> Alignment {
    Alignment {
        phonemes: Vec::new(),
        span: None,
        matched: 0,
        boundaries: 0,
        words: Vec::new(),
        fits: true,
    }
}

/// Align `text` to the audio `env`.
pub fn align(text: &str, language: Language, env: &Envelope) -> Alignment {
    let words = words(text, language);
    let (Some(voiced), false) = (find_voiced(env), words.is_empty()) else {
        return nothing();
    };
    // A text with more sounds than speech holds in this audio is not this audio's text.
    let sounds: usize = words
        .iter()
        .map(|w| w.phonemes.iter().filter(|p| **p != STRESS).count())
        .sum();
    if sounds as f64 > MAX_SOUNDS_PER_SECOND * (voiced.end - voiced.start).max(1.0) {
        return Alignment {
            span: Some((voiced.start, voiced.end)),
            fits: false,
            ..nothing()
        };
    }
    let silence: f64 = voiced.valleys.iter().map(Valley::duration).sum();
    let total = (voiced.end - voiced.start - silence).max(env.hop);
    let total_weight: f64 = words.iter().map(|w| w.weight).sum();
    let valleys = Valleys::new(voiced.start, &voiced.valleys);
    // Where the text puts the end of each word but the last.
    let mut cumulative = 0.0;
    let boundaries: Vec<Boundary> = words[..words.len() - 1]
        .iter()
        .map(|w| {
            cumulative += w.weight;
            Boundary {
                at: total * cumulative / total_weight,
                strong: w.pause.is_some(),
            }
        })
        .collect();
    let pairs = match_boundaries(&boundaries, &valleys.positions, total / words.len() as f64);
    let mut knots = vec![(0.0, 0.0)];
    knots.extend(
        pairs
            .iter()
            .map(|(i, j)| (boundaries[*i].at, valleys.positions[*j])),
    );
    knots.push((total, total));
    let warp = Warp::new(knots);
    // The silence each word's end was paired with.
    let mut paired: Vec<Option<usize>> = vec![None; words.len()];
    for (i, j) in &pairs {
        paired[*i] = Some(*j);
    }

    let mut phonemes = Vec::new();
    let mut starts = Vec::with_capacity(words.len());
    let mut word_start = 0.0;
    for (index, word) in words.iter().enumerate() {
        let word_end = word_start + total * word.weight / total_weight;
        let mut spent = 0.0;
        let first = phonemes.len();
        for p in &word.phonemes {
            let x0 = word_start + (word_end - word_start) * spent / word.weight;
            spent += weight(*p);
            let x1 = word_start + (word_end - word_start) * spent / word.weight;
            let t0 = valleys.to_clip(voiced.start, warp.apply(x0), true);
            let t1 = valleys.to_clip(voiced.start, warp.apply(x1), false).max(t0);
            phonemes.push(Phoneme { p: *p, t0, t1 });
        }
        starts.push(WordStart {
            t: phonemes.get(first).map_or(voiced.start, |p| p.t0),
            word: word.text.clone(),
            prev_ends_clause: index
                .checked_sub(1)
                .is_some_and(|k| words[k].pause.is_some()),
        });
        if let Some(pause) = word.pause.filter(|p| is_pause(*p)) {
            let (t0, t1) = match paired.get(index).copied().flatten() {
                Some(j) => (valleys.all[j].start, valleys.all[j].end),
                None => {
                    let end = phonemes.last().map_or(voiced.end, |p| p.t1);
                    (end, end)
                }
            };
            phonemes.push(Phoneme { p: pause, t0, t1 });
        }
        word_start = word_end;
    }
    Alignment {
        phonemes,
        span: Some((voiced.start, voiced.end)),
        matched: pairs.len(),
        boundaries: boundaries.len(),
        words: starts,
        fits: true,
    }
}
