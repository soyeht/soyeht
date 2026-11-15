//! The loudness of a voice per hop, in dB, and the gain it gives a vowel's opening.
//!
//! A TTS clip is known before it plays, so its envelope is computed once. Ported from
//! `envelopeDb` and the `level` of `clipTrack` in `rig/runtime/mouth.js` (Apache-2.0, the same
//! authors).

use serde::{Deserialize, Serialize};

/// The default hop of an envelope, in seconds.
pub const HOP: f64 = 0.02;

/// Frames below this level are silence when the reference level is found.
const SILENCE_DB: f64 = -50.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// One value per hop: `10 * log10(mean square + 1e-12)`.
    pub db: Vec<f64>,
    pub hop: f64,
}

impl Envelope {
    /// The envelope of mono PCM samples (-1..1) at `sample_rate`, one value per `hop` seconds.
    pub fn from_pcm(samples: &[f32], sample_rate: u32, hop: f64) -> Self {
        let n = ((f64::from(sample_rate) * hop).round() as usize).max(1);
        let db = samples
            .chunks(n)
            .map(|chunk| {
                let energy: f64 =
                    chunk.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / chunk.len() as f64;
                10.0 * (energy + 1e-12).log10()
            })
            .collect();
        Self { db, hop }
    }

    /// The level that counts as "the voice" in this clip: the 90th percentile of the frames that
    /// are not silence. Loudness is measured against it.
    pub fn reference_db(&self) -> f64 {
        let mut loud: Vec<f64> = self
            .db
            .iter()
            .copied()
            .filter(|v| *v > SILENCE_DB)
            .collect();
        loud.sort_by(f64::total_cmp);
        loud.get((loud.len() as f64 * 0.9).floor() as usize)
            .copied()
            .unwrap_or(-20.0)
    }

    /// The gain for the window `t0..t1`: about 0.5 for a soft window to 1.2 for a loud one.
    pub fn gain(&self, reference: f64, t0: f64, t1: f64) -> f64 {
        if self.db.is_empty() || self.hop <= 0.0 {
            return 1.0;
        }
        let last = self.db.len() - 1;
        let i0 = (t0 / self.hop).floor().max(0.0) as usize;
        let i1 = ((t1 / self.hop).floor().max(0.0) as usize)
            .max(i0)
            .min(last);
        // A window past the end of the clip has no voice in it.
        let loudest = self
            .db
            .get(i0..=i1)
            .map_or(-100.0, |w| w.iter().copied().fold(-100.0, f64::max));
        (1.0 + (loudest - reference) / 16.0).clamp(0.5, 1.2)
    }
}
