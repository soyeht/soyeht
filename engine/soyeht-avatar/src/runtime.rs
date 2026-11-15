//! The whole performance of one avatar: the mouth, the acting layer and the clip that is playing.
//! The host calls `step` once per frame with the position of the audio it is actually playing
//! (the audio clock), and draws what comes back.

use crate::acting::{Acting, ActingFrame, ActingInput, State, face_scale};
use crate::acting_tune::{ActingTune, ExpressionTune};
use crate::clip::{Clip, energy_target};
use crate::envelope::sanitize_db;
use crate::mouth::Mouth;
use crate::package::Package;
use crate::params::{MorphWeights, Params};
use crate::tune::Tune;

/// The chin drops this many reference pixels at the widest opening (the lab's `JAW_PX`).
const JAW_PX: f64 = 4.6;

/// The audio that plays now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Audio {
    /// The position in the clip, in seconds, of the sound coming out of the speakers now.
    pub position: f64,
    /// Its level in dB, when the host measures it (used to accent speech without word timings,
    /// and as the whole input of the energy-only tier).
    pub db: Option<f64>,
}

/// What to draw this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub mouth: Params,
    /// The weights of the nine drawings' lip contours (`MorphWeights::all`), summing to 1.
    pub weights: MorphWeights,
    pub acting: ActingFrame,
}

pub struct Runtime {
    lead: f64,
    expression: ExpressionTune,
    mouth: Mouth,
    acting: Acting,
    clip: Option<Clip>,
    /// How many words of the clip the audio has reached.
    words_reached: usize,
}

impl Runtime {
    /// A runtime for an avatar: the acting constants and the size of the face are the package's.
    pub fn new(package: &Package, tune: Tune, seed: u64) -> Self {
        let scale = face_scale(package.warp.head.radii);
        Self::with_scale(tune, package.acting.clone(), seed, scale)
    }

    /// A runtime for a face `scale` times the reference face (`face_scale`).
    pub(crate) fn with_scale(tune: Tune, acting: ActingTune, seed: u64, scale: f64) -> Self {
        Self {
            lead: tune.lead,
            expression: acting.expression.clone(),
            mouth: Mouth::new(tune),
            acting: Acting::new(acting, seed, scale),
            clip: None,
            words_reached: 0,
        }
    }

    /// Use this clip's phonemes for the audio that plays from now on. Without a clip the mouth is
    /// driven by the level alone.
    pub fn set_clip(&mut self, clip: Option<Clip>) {
        self.clip = clip;
        self.words_reached = 0;
    }

    /// The mouth target for the audio that plays: the clip's, and when the clip has nothing to say
    /// while the audio is loud (a text the letter rules could not read, a pause the text did not
    /// have) the level's, so a voice never plays over a shut mouth.
    fn speech_target(&self, audio: Audio) -> Option<Params> {
        let clip_target = self
            .clip
            .as_ref()
            .filter(|_| audio.position.is_finite())
            .and_then(|clip| clip.target_at(audio.position + self.lead));
        clip_target.or_else(|| audio.db.and_then(energy_target))
    }

    /// The resting expression of a state, while nothing is said: a target of the continuous mouth.
    fn expression_target(&self, state: State) -> Option<Params> {
        match state {
            State::Listening => Some(self.expression.listening),
            State::Thinking => Some(self.expression.thinking),
            State::Idle | State::Speaking => None,
        }
    }

    /// Raise the accent of the word the audio has just reached (only the latest, when a long
    /// frame crosses several), and follow a seek backwards.
    fn word_accents(&mut self, audio: Option<Audio>) {
        let (Some(clip), Some(audio)) = (&self.clip, audio) else {
            return;
        };
        if !audio.position.is_finite() {
            return;
        }
        let reached = clip.words().partition_point(|w| w.t <= audio.position);
        if reached > self.words_reached {
            let word = &clip.words()[reached - 1];
            self.acting.word_start(
                &word.word,
                reached - 1,
                clip.words().len(),
                word.prev_ends_clause,
                clip.text(),
            );
        }
        self.words_reached = reached;
    }

    /// Advance by `dt` seconds. `state` is the conversation's state; `audio` is `None` when
    /// nothing is playing. The mouth steps first: the acting layer drops the chin with it.
    pub fn step(&mut self, dt: f64, state: State, audio: Option<Audio>) -> Frame {
        let audio = audio.map(|a| Audio {
            db: a.db.map(sanitize_db),
            ..a
        });
        let mouth = match (audio, self.expression_target(state)) {
            (Some(a), _) => self.mouth.step(dt, self.speech_target(a)),
            // An expression is not speech: it does not restart the silence.
            (None, Some(expression)) => self.mouth.hold(dt, expression),
            (None, None) => self.mouth.step(dt, None),
        };
        self.word_accents(audio);
        // The chin follows the opening, and drops a little less when the lips are pressed.
        let jaw = mouth.o * JAW_PX * (0.8 + 0.2 * (1.0 - mouth.c));
        let acting = self.acting.step(
            dt,
            ActingInput {
                state,
                voice: audio.is_some(),
                db: audio.and_then(|a| a.db),
                words_known: self.clip.as_ref().is_some_and(|c| !c.words().is_empty()),
                jaw,
            },
        );
        Frame {
            mouth,
            weights: self.mouth.weights(),
            acting,
        }
    }
}
