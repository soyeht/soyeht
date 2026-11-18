//! What is said over the avatar or over the wait for its reply, before it is believed (contract 20,
//! section 2): a listener's noise or the avatar's own voice heard by the microphone does not take
//! the turn.

use crate::text::tokens;

/// Whether a text repeats words of the last sentences played.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Echo {
    Yes,
    /// One word that the sentences hold: it decides nothing as a partial text.
    Maybe,
    No,
}

/// The listener's noises, as the words they are made of.
pub(crate) struct Continuers(Vec<Vec<String>>);

impl Continuers {
    pub(crate) fn new(entries: &[String]) -> Self {
        let words = entries
            .iter()
            .map(|entry| tokens(entry))
            .filter(|words| !words.is_empty())
            .collect();
        Self(words)
    }

    /// The whole text is made of listener noises, or is the start of one.
    pub(crate) fn contains_only(&self, words: &[String]) -> bool {
        let mut index = 0;
        while index < words.len() {
            let rest = &words[index..];
            match self.0.iter().find(|entry| rest.starts_with(entry)) {
                Some(entry) => index += entry.len(),
                None => return self.0.iter().any(|entry| entry.starts_with(rest)),
            }
        }
        true
    }
}

/// Whether the words repeat the sentences played: the speaker heard by the microphone. Two words or
/// more that appear together are an echo (the last may be unfinished). One word that appears in
/// them decides nothing while it is a partial text, and is the person's only once it is final.
pub(crate) fn echo_of(played: &[Vec<String>], words: &[String]) -> Echo {
    let heard: Vec<&String> = played.iter().flatten().collect();
    let [only] = words else {
        return if appears_together(&heard, words) {
            Echo::Yes
        } else {
            Echo::No
        };
    };
    if heard.iter().any(|word| word.starts_with(only.as_str())) {
        Echo::Maybe
    } else {
        Echo::No
    }
}

fn appears_together(heard: &[&String], words: &[String]) -> bool {
    let Some((last, before)) = words.split_last() else {
        return false;
    };
    heard.windows(words.len()).any(|window| {
        window
            .iter()
            .zip(before)
            .all(|(heard, word)| *heard == word)
            && window
                .last()
                .is_some_and(|heard| heard.starts_with(last.as_str()))
    })
}
