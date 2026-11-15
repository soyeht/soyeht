//! The whole performance of one avatar: the mouth, the acting layer and the clip that is playing.
//! The host calls `step` once per frame with the position of the audio it is actually playing
//! (the audio clock), and draws what comes back.

use crate::acting::{Acting, ActingFrame, ActingInput, State};
use crate::acting_tune::ActingTune;
use crate::clip::{Clip, energy_target};
use crate::mouth::Mouth;
use crate::params::{MorphWeights, Params};
use crate::tune::Tune;

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
    mouth: Mouth,
    acting: Acting,
    clip: Option<Clip>,
}

impl Runtime {
    pub fn new(tune: Tune, acting: ActingTune, seed: u64) -> Self {
        Self {
            lead: tune.lead,
            mouth: Mouth::new(tune),
            acting: Acting::new(acting, seed),
            clip: None,
        }
    }

    /// Use this clip's phonemes for the audio that plays from now on. Without a clip the mouth is
    /// driven by the level alone.
    pub fn set_clip(&mut self, clip: Option<Clip>) {
        self.clip = clip;
    }

    pub fn acting_mut(&mut self) -> &mut Acting {
        &mut self.acting
    }

    /// Advance by `dt` seconds. `state` is the conversation's state; `audio` is `None` when
    /// nothing is playing.
    pub fn step(&mut self, dt: f64, state: State, audio: Option<Audio>) -> Frame {
        let target = audio.and_then(|a| match &self.clip {
            // The mouth leads the sound a little: it looks ahead by `lead`.
            Some(clip) => clip.target_at(a.position + self.lead),
            None => a.db.and_then(energy_target),
        });
        let acting = self.acting.step(
            dt,
            ActingInput {
                state,
                voice: audio.is_some(),
                db: audio.and_then(|a| a.db),
                words_known: self.clip.is_some(),
                jaw: 0.0,
            },
        );
        let mouth = self.mouth.step(dt, target);
        Frame {
            mouth,
            weights: self.mouth.weights(),
            acting,
        }
    }
}
