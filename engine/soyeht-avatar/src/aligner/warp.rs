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

/// A best score and the candidate that gave it.
type Best = (f64, Option<usize>);

/// A Fenwick tree of running maxima over silence indices: the best chain that ends before index
/// `j`.
struct PrefixMax(Vec<Best>);

impl PrefixMax {
    fn new(len: usize) -> Self {
        Self(vec![(0.0, None); len + 1])
    }

    fn update(&mut self, at: usize, best: Best) {
        let mut k = at + 1;
        while k < self.0.len() {
            if best.0 > self.0[k].0 {
                self.0[k] = best;
            }
            k += k.isolate_lowest_one();
        }
    }

    /// The best over the indices `0..at`.
    fn query(&self, at: usize) -> Best {
        let (mut k, mut out) = (at, (0.0, None));
        while k > 0 {
            if self.0[k].0 > out.0 {
                out = self.0[k];
            }
            k -= k.isolate_lowest_one();
        }
        out
    }
}

/// Pair boundaries with silences (`positions`, in voiced time), both increasing. The result lists
/// `(boundary index, silence index)` in order. `mean_word` is the mean duration of a word.
///
/// Only a boundary and a silence within the tolerance can be paired, so the candidates are few
/// (each boundary has the handful of silences near it) and the best chain is found over them with
/// a Fenwick tree: the work grows with the number of candidates, not with the product of the
/// number of words and the number of silences.
pub(crate) fn match_boundaries(
    boundaries: &[Boundary],
    positions: &[f64],
    mean_word: f64,
) -> Vec<(usize, usize)> {
    let tolerance = MIN_TOLERANCE.max(TOLERANCE_SHARE * mean_word);
    // (boundary, silence, score), in order of boundary and then silence.
    let mut candidates: Vec<(usize, usize, f64)> = Vec::new();
    for (i, b) in boundaries.iter().enumerate() {
        let near = positions.partition_point(|p| *p < b.at - tolerance);
        for (j, p) in positions.iter().enumerate().skip(near) {
            let d = (b.at - p).abs();
            if *p > b.at + tolerance {
                break;
            }
            let bonus = if b.strong { STRONG_BONUS } else { 0.0 };
            candidates.push((i, j, 1.0 - d / tolerance + bonus));
        }
    }
    let mut chain: Vec<Best> = Vec::with_capacity(candidates.len());
    let mut tree = PrefixMax::new(positions.len());
    let mut group_start = 0;
    while group_start < candidates.len() {
        let boundary = candidates[group_start].0;
        let group_end = candidates[group_start..]
            .iter()
            .position(|c| c.0 != boundary)
            .map_or(candidates.len(), |n| group_start + n);
        // A chain through this boundary extends one that ended at an earlier boundary and an
        // earlier silence: the tree holds only the earlier boundaries until the group is done.
        for (_, j, score) in &candidates[group_start..group_end] {
            let (before, parent) = tree.query(*j);
            chain.push((before + score, parent));
        }
        for k in group_start..group_end {
            tree.update(candidates[k].1, (chain[k].0, Some(k)));
        }
        group_start = group_end;
    }
    let mut at = chain
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.0.total_cmp(&b.1.0))
        .map(|(k, _)| k);
    let mut pairs = Vec::new();
    while let Some(k) = at {
        pairs.push((candidates[k].0, candidates[k].1));
        at = chain[k].1;
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
