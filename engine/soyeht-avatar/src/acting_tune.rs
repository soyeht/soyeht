//! The numbers of the acting layer: the defaults are the lab's (`static/sabio-acting.js`, TUNE).
//! A package may override any of them; a field it leaves out keeps the default. Times are
//! seconds, angles are degrees, and distances are pixels of a picture `REFERENCE_SIZE` (1024)
//! pixels tall: the acting layer scales them to the real canvas, so a tune does not depend on the
//! size of the picture it is used with.

use serde::{Deserialize, Serialize};

use crate::params::Params;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ActingTune {
    pub blink: BlinkTune,
    pub gaze: GazeTune,
    pub head: HeadTune,
    pub brow: BrowTune,
    pub breath: BreathTune,
    pub stress: StressTune,
    pub expression: ExpressionTune,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BlinkTune {
    /// Range of the gap between blinks.
    pub gap: [f64; 2],
    pub gap_listening: [f64; 2],
    pub double_chance: f64,
    /// Chance that a blink goes with a glance.
    pub on_gaze_shift: f64,
    /// Half, closed, half: how long each lid drawing is held (close fast, hold, open slower).
    pub seq: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GazeTune {
    pub idle_gap: [f64; 2],
    pub hold_away: [f64; 2],
    pub speaking_away_chance: f64,
    pub thinking_hold: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HeadTune {
    pub sway_rot: f64,
    pub sway_x: f64,
    pub sway_y: f64,
    pub listen_tilt: f64,
    pub listen_x: f64,
    pub think_tilt: f64,
    pub think_y: f64,
    pub nod_speak: f64,
    pub nod_start: f64,
    pub nod_listen: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BrowTune {
    pub stress: f64,
    pub start: f64,
    pub thinking: f64,
    pub listening: f64,
    pub idle_flick: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BreathTune {
    pub period: f64,
    pub period_speaking: f64,
    pub amount: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StressTune {
    /// A voiced level this far over the recent level is an accent (dB).
    pub db_over: f64,
    pub min_gap: f64,
    /// A word of at least this many letters is a content word.
    pub content_len: usize,
    pub chance: f64,
    pub word_gap: f64,
}

impl Default for BlinkTune {
    fn default() -> Self {
        Self {
            gap: [2.2, 5.8],
            gap_listening: [3.2, 7.5],
            double_chance: 0.14,
            on_gaze_shift: 0.35,
            seq: [0.042, 0.07, 0.058],
        }
    }
}

impl Default for GazeTune {
    fn default() -> Self {
        Self {
            idle_gap: [1.8, 4.6],
            hold_away: [0.45, 1.3],
            speaking_away_chance: 0.45,
            thinking_hold: [1.2, 2.6],
        }
    }
}

impl Default for HeadTune {
    fn default() -> Self {
        Self {
            sway_rot: 0.45,
            sway_x: 1.4,
            sway_y: 0.85,
            listen_tilt: -2.1,
            listen_x: 1.6,
            think_tilt: 1.0,
            think_y: -1.4,
            nod_speak: 13.0,
            nod_start: 14.0,
            nod_listen: 9.0,
        }
    }
}

impl Default for BrowTune {
    fn default() -> Self {
        Self {
            stress: 5.0,
            start: 4.8,
            thinking: 4.0,
            listening: 2.0,
            idle_flick: 2.8,
        }
    }
}

impl Default for BreathTune {
    fn default() -> Self {
        Self {
            period: 4.4,
            period_speaking: 3.7,
            amount: 1.0,
        }
    }
}

impl Default for StressTune {
    fn default() -> Self {
        Self {
            db_over: 7.0,
            min_gap: 0.9,
            content_len: 6,
            chance: 0.75,
            word_gap: 0.65,
        }
    }
}

/// The expression of the resting mouth, as targets of the continuous mouth (not as pictures: a
/// mouth drawing is never swapped in). While nothing is said, a listening avatar holds a slight
/// smile and a thinking one a pressed, slightly pursed "hm"; the mouth morphs there and back like
/// everywhere else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExpressionTune {
    pub listening: Params,
    pub thinking: Params,
}

impl Default for ExpressionTune {
    fn default() -> Self {
        Self {
            listening: Params::new(0.05, 0.7, 0.0, 0.0, 0.0),
            thinking: Params::new(0.0, 0.35, 0.3, 0.6, 0.0),
        }
    }
}
