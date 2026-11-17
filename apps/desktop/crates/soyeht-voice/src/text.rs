//! How the voice loop reads text: words in Unicode NFC, lowercase, as runs of letters, marks and
//! digits, and the character classes the speech rules share (contract 20, sections 2 and 3).

use icu_normalizer::ComposingNormalizerBorrowed;
use icu_properties::{CodePointMapData, props::GeneralCategory};
use soyeht_protocol::client_policy::{VOICE_PICTOGRAPHS, VOICE_SCRIPTS_WITHOUT_BLANKS};
use std::sync::LazyLock;

/// Code point ranges from a list of the policy (`1F000-1FAFF|20E3`: a range or one code point, in
/// hexadecimal, separated by `|`).
pub(crate) struct ScalarRanges {
    ranges: Vec<(u32, u32)>,
}

impl ScalarRanges {
    fn parse(policy: &str) -> Self {
        let ranges = policy
            .split('|')
            .filter_map(|entry| {
                let (first, last) = entry.split_once('-').unwrap_or((entry, entry));
                Some((hex(first)?, hex(last)?))
            })
            .collect();
        Self { ranges }
    }

    pub(crate) fn contains(&self, character: char) -> bool {
        let value = u32::from(character);
        self.ranges
            .iter()
            .any(|&(first, last)| (first..=last).contains(&value))
    }
}

fn hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text, 16).ok()
}

static BLANKLESS: LazyLock<ScalarRanges> =
    LazyLock::new(|| ScalarRanges::parse(VOICE_SCRIPTS_WITHOUT_BLANKS));
static PICTOGRAPHS: LazyLock<ScalarRanges> =
    LazyLock::new(|| ScalarRanges::parse(VOICE_PICTOGRAPHS));

/// A script whose words are not set apart by blanks (Han, kana, Thai, Lao, Khmer, Myanmar).
pub(crate) fn is_blankless(character: char) -> bool {
    BLANKLESS.contains(character)
}

/// Emoji and symbols a voice would read out as their names, and the joiners between them.
pub(crate) fn is_pictograph(character: char) -> bool {
    PICTOGRAPHS.contains(character)
}

fn category(character: char) -> GeneralCategory {
    CodePointMapData::<GeneralCategory>::new().get(character)
}

/// Category L.
pub(crate) fn is_letter(character: char) -> bool {
    matches!(
        category(character),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
    )
}

/// Category N.
pub(crate) fn is_number(character: char) -> bool {
    matches!(
        category(character),
        GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
    )
}

/// Category M: the combining marks that Rust's `char::is_alphanumeric` leaves out.
fn is_mark(character: char) -> bool {
    matches!(
        category(character),
        GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
    )
}

/// What the Unicode `Zs` category and the tab call a blank: the characters Foundation's
/// `whitespaces` set trims. A newline is not one.
pub(crate) fn trim_blanks(text: &str) -> &str {
    text.trim_matches(|character: char| {
        character == '\t' || category(character) == GeneralCategory::SpaceSeparator
    })
}

/// The words of a text, for comparing what was said with what was played: Unicode NFC, lowercase,
/// a word being a run of letters, marks and digits and anything else a break. Each character of a
/// script with no blanks is a word of its own.
pub(crate) fn tokens(text: &str) -> Vec<String> {
    let composed = ComposingNormalizerBorrowed::new_nfc().normalize(text);
    let mut words = Vec::new();
    let mut current = String::new();
    for character in composed.to_lowercase().chars() {
        if is_blankless(character) {
            words.extend((!current.is_empty()).then(|| std::mem::take(&mut current)));
            words.push(character.to_string());
        } else if is_letter(character) || is_mark(character) || is_number(character) {
            current.push(character);
        } else {
            words.extend((!current.is_empty()).then(|| std::mem::take(&mut current)));
        }
    }
    words.extend((!current.is_empty()).then_some(current));
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        tokens(text)
    }

    #[test]
    fn every_entry_of_a_policy_list_is_a_range() {
        for policy in [VOICE_SCRIPTS_WITHOUT_BLANKS, VOICE_PICTOGRAPHS] {
            let ranges = ScalarRanges::parse(policy);
            assert_eq!(ranges.ranges.len(), policy.split('|').count(), "{policy}");
        }
    }

    #[test]
    fn words_are_compared_composed_and_in_lowercase() {
        assert_eq!(words("Cafe\u{301} NOIR"), words("caf\u{e9} noir"));
        assert_eq!(words("Cafe\u{301}"), ["caf\u{e9}"]);
    }

    #[test]
    fn a_word_is_a_run_of_letters_marks_and_digits() {
        // No precomposed form of "q with a tilde" exists: the mark stays in its word.
        assert_eq!(words("q\u{303}ue"), ["q\u{303}ue"]);
        assert_eq!(
            words("Room 42, it's 3:30."),
            ["room", "42", "it", "s", "3", "30"]
        );
    }

    #[test]
    fn each_character_of_a_script_with_no_blanks_is_a_word() {
        assert_eq!(words("今日は ok"), ["今", "日", "は", "ok"]);
    }
}
