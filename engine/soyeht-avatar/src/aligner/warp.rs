//! Matching the word boundaries of a text to the silences of the audio.
//!
//! The text predicts where each word ends (in proportion to its sounds, in voiced time, that is
//! the time left when the silences are taken out). The audio shows where the silences really
//! are. A monotone matching pairs the boundaries with the silences that are close enough, and the
//! pairs become anchors of a piecewise linear time warp, so a word is stretched to fit between
//! the silences that surround it.

/// A boundary the text predicts, in voiced time, and whether the text has a pause mark there.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Boundary {
    pub at: f64,
    pub strong: bool,
}

/// How much a pause mark is worth when it meets a silence, beside the closeness score of 0..=1.
const STRONG_BONUS: f64 = 0.5;
/// The nearest a boundary must be to a silence to be paired with it (seconds, at least).
const MIN_TOLERANCE: f64 = 0.12;
/// ... and as a share of the mean word duration.
const TOLERANCE_SHARE: f64 = 0.35;

/// Pair boundaries with silences (`positions`, in voiced time), both increasing. The result lists
/// `(boundary index, silence index)` in order. `mean_word` is the mean duration of a word.
pub(crate) fn match_boundaries(
    boundaries: &[Boundary],
    positions: &[f64],
    mean_word: f64,
) -> Vec<(usize, usize)> {
    let tolerance = MIN_TOLERANCE.max(TOLERANCE_SHARE * mean_word);
    let score = |i: usize, j: usize| -> Option<f64> {
        let d = (boundaries[i].at - positions[j]).abs();
        (d <= tolerance).then(|| {
            1.0 - d / tolerance
                + if boundaries[i].strong {
                    STRONG_BONUS
                } else {
                    0.0
                }
        })
    };
    let (n, m) = (boundaries.len(), positions.len());
    let mut best = vec![vec![0.0f64; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            let skip = best[i - 1][j].max(best[i][j - 1]);
            best[i][j] = score(i - 1, j - 1).map_or(skip, |s| skip.max(best[i - 1][j - 1] + s));
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        let here = best[i][j];
        if here == best[i - 1][j] {
            i -= 1;
        } else if here == best[i][j - 1] {
            j -= 1;
        } else {
            pairs.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        }
    }
    pairs.reverse();
    pairs
}

/// A monotone piecewise linear map through knots `(from, to)`, increasing in both, with the ends
/// fixed by the first and last knot.
#[derive(Debug, Clone)]
pub(crate) struct Warp {
    knots: Vec<(f64, f64)>,
}

impl Warp {
    pub fn new(mut knots: Vec<(f64, f64)>) -> Self {
        knots.dedup_by(|b, a| b.0 <= a.0 + 1e-9 || b.1 <= a.1 + 1e-9);
        Self { knots }
    }

    pub fn apply(&self, x: f64) -> f64 {
        let k = self.knots.partition_point(|(from, _)| *from <= x);
        match (
            k.checked_sub(1).and_then(|a| self.knots.get(a)),
            self.knots.get(k),
        ) {
            (Some(a), Some(b)) => a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0),
            (Some(a), None) => a.1 + (x - a.0),
            (None, Some(b)) => b.1 + (x - b.0),
            (None, None) => x,
        }
    }
}
