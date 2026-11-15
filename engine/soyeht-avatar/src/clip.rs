//! A clip is one stretch of speech whose phonemes are known before it plays: the target track is
//! built once and read by the audio clock. It may carry the start of each word, so the acting
//! layer can accent the content words.

use crate::envelope::{Envelope, sanitize_db};
use crate::params::Params;
use crate::phoneme::Phoneme;
use crate::track::{Target, build_track, target_at};
use crate::tune::Tune;

/// When a word of the text starts, in clip time.
#[derive(Debug, Clone, PartialEq)]
pub struct WordStart {
    pub t: f64,
    pub word: String,
    /// The word before it ended a clause (it was followed by a pause mark).
    pub prev_ends_clause: bool,
}

/// The most targets a second of speech can hold. People say about fifteen sounds a second at
/// speed; a track with more than twice that is a text that does not belong to its audio (a reply
/// much longer than the voice that plays), and a mouth that tried to follow it would be a blur of
/// closures.
const MAX_TARGETS_PER_SECOND: f64 = 30.0;

#[derive(Debug, Clone)]
pub struct Clip {
    track: Vec<Target>,
    /// The track is denser than speech can be: it is not followed.
    unspeakable: bool,
    text: String,
    words: Vec<WordStart>,
}

impl Clip {
    /// Build the track from timed phonemes, with the voice envelope when there is one (without
    /// it every vowel has the plain gain).
    pub fn new(phonemes: &[Phoneme], envelope: Option<&Envelope>, tune: &Tune) -> Self {
        let track = match envelope {
            Some(env) => {
                let reference = env.reference_db();
                build_track(phonemes, &|t0, t1| env.gain(reference, t0, t1), tune)
            }
            None => build_track(phonemes, &|_, _| 1.0, tune),
        };
        // The span of the speech, at least a second, so a short clip may hold a few sounds.
        let span = match (track.first(), track.last()) {
            (Some(first), Some(last)) => (last.t1 - first.t0).max(1.0),
            _ => 1.0,
        };
        let unspeakable = track.len() as f64 > MAX_TARGETS_PER_SECOND * span;
        Self {
            track,
            unspeakable,
            text: String::new(),
            words: Vec::new(),
        }
    }

    /// Say which words the clip holds and when each starts (from the TTS timings, or from
    /// `Alignment::words`), and the whole text, so the runtime raises the accents itself.
    pub fn with_words(mut self, text: &str, mut words: Vec<WordStart>) -> Self {
        words.retain(|w| w.t.is_finite());
        words.sort_by(|a, b| a.t.total_cmp(&b.t));
        self.text = text.to_string();
        self.words = words;
        self
    }

    pub fn track(&self) -> &[Target] {
        &self.track
    }

    pub(crate) fn words(&self) -> &[WordStart] {
        &self.words
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The mouth target at the clip time `t` (the position of the audio actually playing). A
    /// clip with more sounds than speech can hold has none: the runtime then follows the level.
    pub fn target_at(&self, t: f64) -> Option<Params> {
        if self.unspeakable {
            return None;
        }
        target_at(&self.track, t)
    }
}

/// Below this level the voice is silence (dB).
const ENERGY_FLOOR_DB: f64 = -50.0;
/// The span of levels over which the opening goes from closed to its widest (dB).
const ENERGY_RANGE_DB: f64 = 30.0;

/// The floor tier of ADR 0166 (the mouth input is energy only): no text and no timings, so the
/// opening follows the loudness and the lips take a neutral shape. `None` is silence. It is also
/// what the mouth does when a clip has nothing to say while the audio is loud, such as a text the
/// letter rules cannot read.
pub fn energy_target(db: f64) -> Option<Params> {
    let db = sanitize_db(db);
    (db > ENERGY_FLOOR_DB).then(|| {
        let open = ((db - ENERGY_FLOOR_DB) / ENERGY_RANGE_DB).clamp(0.0, 1.0);
        Params::new(open * 0.8, 0.55, 0.0, 0.0, 0.0)
    })
}
