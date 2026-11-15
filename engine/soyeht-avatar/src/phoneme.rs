//! Phonemes as the mouth sees them: IPA letters with the lab's few extras (`ʤ ʧ` affricates, `W`
//! a dark l that sounds like u, `æ` a reduced final a), a stress mark and the pause marks.

use serde::{Deserialize, Serialize};

/// One timed phoneme, in seconds of clip time. A stress mark `ˈ` has `t0 == t1`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Phoneme {
    pub p: char,
    pub t0: f64,
    pub t1: f64,
}

pub(crate) const STRESS: char = 'ˈ';

/// What a phoneme does to the lips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// A vowel: opening, spread and rounding from the table.
    Vowel,
    /// A semivowel (`w`, `W`, `j`): a vowel shape, a little less open.
    Semi,
    /// A tongue or throat consonant, or a vowel too short to move the lips: the lips follow the
    /// neighbouring vowels.
    Transparent,
    /// `p b m`: the lips close.
    Bilabial,
    /// `f v`: the lower lip meets the upper teeth.
    Labiodental,
    /// A pause mark: the lips rest together.
    Pause,
}

/// Vowel targets `[opening, width, rounding]`.
pub(crate) fn vowel(p: char) -> Option<[f64; 3]> {
    Some(match p {
        'a' => [1.0, 0.55, 0.1],
        'ɑ' => [0.95, 0.5, 0.1],
        'ɐ' => [0.7, 0.5, 0.05],
        'æ' => [0.65, 0.55, 0.0],
        'ə' => [0.5, 0.5, 0.0],
        'ɛ' => [0.8, 0.8, 0.0],
        'e' => [0.6, 0.88, 0.0],
        'i' => [0.3, 1.0, 0.0],
        'ɪ' => [0.35, 0.85, 0.0],
        'y' => [0.3, 0.95, 0.0],
        'ɔ' => [0.88, 0.25, 0.7],
        'o' => [0.66, 0.15, 0.85],
        'u' => [0.3, 0.05, 1.0],
        'ʊ' => [0.35, 0.3, 0.6],
        'w' => [0.2, 0.0, 1.0],
        'W' => [0.25, 0.1, 0.85],
        'j' => [0.2, 0.95, 0.0],
        _ => return None,
    })
}

pub(crate) fn is_semi(p: char) -> bool {
    matches!(p, 'w' | 'W' | 'j')
}

pub(crate) fn is_bilabial(p: char) -> bool {
    matches!(p, 'p' | 'b' | 'm')
}

pub(crate) fn is_labiodental(p: char) -> bool {
    matches!(p, 'f' | 'v')
}

pub(crate) fn is_palatal(p: char) -> bool {
    matches!(p, 'ʃ' | 'ʒ' | 'ʧ' | 'ʤ' | 'ɲ' | 'ʎ')
}

pub(crate) fn is_sibilant(p: char) -> bool {
    matches!(p, 's' | 'z')
}

pub(crate) fn is_pause(p: char) -> bool {
    matches!(p, '.' | ',' | '!' | '?' | ';' | ':' | '…' | '—' | '-')
}

/// Marks that carry no sound of their own.
pub(crate) fn is_skipped(p: char) -> bool {
    matches!(p, 'ˈ' | 'ˌ' | '˜' | '\u{303}' | 'ː' | ' ')
}
