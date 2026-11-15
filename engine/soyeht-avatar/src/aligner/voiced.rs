//! Where the voice is: the span of the clip that carries speech and the silences inside it.

use crate::envelope::Envelope;

/// A silence inside the voiced span.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Valley {
    pub start: f64,
    pub end: f64,
}

impl Valley {
    pub fn duration(&self) -> f64 {
        self.end - self.start
    }
}

/// The voiced span of a clip: from the first voiced frame to the end of the last, with the
/// silences of at least `MIN_VALLEY` inside it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Voiced {
    pub start: f64,
    pub end: f64,
    pub valleys: Vec<Valley>,
}

/// A silence shorter than this is a stop closure or a breath of the envelope, not a boundary
/// between words (seconds).
const MIN_VALLEY: f64 = 0.06;
/// The voice threshold sits this far under the reference level, within these bounds (dB).
const BELOW_REFERENCE_DB: f64 = 30.0;
const THRESHOLD_RANGE_DB: (f64, f64) = (-55.0, -35.0);

pub(crate) fn find_voiced(env: &Envelope) -> Option<Voiced> {
    let threshold =
        (env.reference_db() - BELOW_REFERENCE_DB).clamp(THRESHOLD_RANGE_DB.0, THRESHOLD_RANGE_DB.1);
    let voiced: Vec<bool> = env.db.iter().map(|v| *v > threshold).collect();
    let first = voiced.iter().position(|v| *v)?;
    let last = voiced.iter().rposition(|v| *v)?;
    let mut valleys = Vec::new();
    let mut run_start = None;
    for (i, on) in voiced.iter().enumerate().take(last + 1).skip(first) {
        match (on, run_start) {
            (false, None) => run_start = Some(i),
            (true, Some(s)) => {
                let valley = Valley {
                    start: s as f64 * env.hop,
                    end: i as f64 * env.hop,
                };
                if valley.duration() >= MIN_VALLEY {
                    valleys.push(valley);
                }
                run_start = None;
            }
            _ => {}
        }
    }
    Some(Voiced {
        start: first as f64 * env.hop,
        end: (last + 1) as f64 * env.hop,
        valleys,
    })
}
