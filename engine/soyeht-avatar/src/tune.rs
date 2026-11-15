//! Every number worth tuning in the mouth dynamics. The defaults are the validated H0 values of
//! the avatar lab (`rig/runtime/mouth.js`, TUNE); changing one is a change to a mouth the owner
//! approved, so it goes through the checker limits and a look at a contact sheet.

use crate::params::DIMS;

/// Times are seconds; per-parameter arrays are in the order `o, w, r, c, f`.
#[derive(Debug, Clone, PartialEq)]
pub struct Tune {
    /// The mouth leads the audio by this much.
    pub lead: f64,
    /// Smooth times towards a more extreme value.
    pub attack: [f64; DIMS],
    /// Smooth times back towards rest.
    pub release: [f64; DIMS],
    /// A closure target lasts at least this long (about three frames at 24 fps).
    pub min_closure: f64,
    /// A closure starts this much earlier: the lips are together when the burst is heard.
    pub closure_lead: f64,
    /// A vowel shorter than this is not animated; it takes the shape of its neighbours.
    pub min_vowel: f64,
    /// Vowel openings are pushed this far past their plain value.
    pub exaggerate: f64,
    /// Between words the mouth stays at least this open; lips close only for M B P and pauses.
    pub floor: f64,
    /// After this much silence the lips are together.
    pub silence_close: f64,
    /// After this much silence they relax into the resting smile.
    pub silence_rest: f64,
    /// Closure of the lips in a pause.
    pub pause_closure: f64,
    /// Per second: no parameter changes faster than this, so the mouth never changes in one frame.
    pub max_rate: [f64; DIMS],
    /// Smooth time of the slow return to the resting smile after a long silence.
    pub relax: f64,
    /// The morph weights follow the parameters' weights with this smooth time.
    pub weight_smooth: f64,
}

impl Default for Tune {
    fn default() -> Self {
        Self {
            lead: 0.045,
            attack: [0.07, 0.08, 0.085, 0.035, 0.045],
            release: [0.1, 0.095, 0.1, 0.06, 0.06],
            min_closure: 0.11,
            closure_lead: 0.06,
            min_vowel: 0.045,
            exaggerate: 1.3,
            floor: 0.2,
            silence_close: 0.05,
            silence_rest: 1.0,
            pause_closure: 0.9,
            max_rate: [9.0, 7.0, 7.0, 13.0, 12.0],
            relax: 0.22,
            weight_smooth: 0.04,
        }
    }
}
