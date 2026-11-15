//! The mouth as five continuous parameters, and the nine drawings as anchors in that space.
//!
//! A mouth is never a picture. Phonemes are targets in this space; the parameters become morph
//! weights that blend the lip contours of the nine drawings (see `mesh`), so exactly one mouth
//! is ever visible. Ported from `rig/runtime/mouth.js` (Apache-2.0, the same authors).

use serde::{Deserialize, Serialize};

/// The five parameters, each in 0..=1: opening `o` (jaw height), width `w` (0 pursed, 1 spread),
/// rounding `r` (lip protrusion), closure `c` (lips pressed: M, B, P) and lip-teeth contact `f`
/// (F, V).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Params {
    pub o: f64,
    pub w: f64,
    pub r: f64,
    pub c: f64,
    pub f: f64,
}

impl Params {
    /// The resting mouth: closed, a slight smile.
    pub const REST: Params = Params::new(0.0, 0.5, 0.0, 0.0, 0.0);

    pub const fn new(o: f64, w: f64, r: f64, c: f64, f: f64) -> Self {
        Self { o, w, r, c, f }
    }

    pub(crate) fn to_array(self) -> [f64; DIMS] {
        [self.o, self.w, self.r, self.c, self.f]
    }

    pub(crate) fn from_array(a: [f64; DIMS]) -> Self {
        Self::new(a[0], a[1], a[2], a[3], a[4])
    }
}

pub(crate) const DIMS: usize = 5;

pub(crate) fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// The nine drawings of a mouth. `Rest` is the base picture's own mouth; the other eight are the
/// visemes. The order is the order of the weights and of the package's shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    Rest,
    ClosedMbp,
    A,
    E,
    I,
    O,
    U,
    Fv,
    Ldt,
}

impl Shape {
    pub const ALL: [Shape; SHAPES] = [
        Shape::Rest,
        Shape::ClosedMbp,
        Shape::A,
        Shape::E,
        Shape::I,
        Shape::O,
        Shape::U,
        Shape::Fv,
        Shape::Ldt,
    ];

    /// The id the package and the frame spec use for this drawing.
    pub fn id(self) -> &'static str {
        match self {
            Shape::Rest => "rest",
            Shape::ClosedMbp => "closed_MBP",
            Shape::A => "A",
            Shape::E => "E",
            Shape::I => "I",
            Shape::O => "O",
            Shape::U => "U",
            Shape::Fv => "FV",
            Shape::Ldt => "LDT",
        }
    }

    /// Where this drawing sits in the parameter space.
    pub fn anchor(self) -> Params {
        match self {
            Shape::Rest => Params::new(0.0, 0.5, 0.0, 0.0, 0.0),
            Shape::ClosedMbp => Params::new(0.0, 0.45, 0.0, 1.0, 0.0),
            Shape::A => Params::new(1.0, 0.55, 0.1, 0.0, 0.0),
            Shape::E => Params::new(0.4, 0.9, 0.0, 0.0, 0.0),
            Shape::I => Params::new(0.15, 1.0, 0.0, 0.0, 0.0),
            Shape::O => Params::new(0.65, 0.15, 0.8, 0.0, 0.0),
            Shape::U => Params::new(0.2, 0.0, 1.0, 0.0, 0.0),
            Shape::Fv => Params::new(0.15, 0.7, 0.0, 0.0, 1.0),
            Shape::Ldt => Params::new(0.5, 0.65, 0.0, 0.0, 0.0),
        }
    }
}

pub(crate) const SHAPES: usize = 9;
/// The viseme count: every shape but `Rest`.
pub(crate) const VISEMES: usize = SHAPES - 1;

/// Per-dimension distance scale, the exponent of the inverse-distance weights and how many shapes
/// may take part in one mix (the weight of the fifth nearest is the cutoff, which keeps the
/// weights continuous).
const SCALE: [f64; DIMS] = [1.3, 1.0, 1.0, 2.0, 2.2];
const POWER: f64 = 2.5;
const KEEP: usize = 5;

/// The morph weights of the eight visemes. `Rest` takes what is left, so the nine always sum
/// to 1 and are never negative: the mouth passes between shapes by mixing their geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MorphWeights {
    /// `closed_MBP, A, E, I, O, U, FV, LDT`, in the order of `Shape::ALL` after `Rest`.
    pub visemes: [f64; VISEMES],
}

impl MorphWeights {
    /// All nine weights in the order of `Shape::ALL`, non-negative and summing to 1.
    pub fn all(&self) -> [f64; SHAPES] {
        let mut out = [0.0; SHAPES];
        let mut sum = 0.0;
        for (slot, w) in out[1..].iter_mut().zip(self.visemes) {
            *slot = w.max(0.0);
            sum += *slot;
        }
        if sum < 1.0 {
            out[0] = 1.0 - sum;
        } else {
            for w in out.iter_mut() {
                *w /= sum;
            }
        }
        out
    }
}

/// Parameters to morph weights: inverse-distance weights with a compact, continuous support.
pub fn weights_from_params(q: Params) -> MorphWeights {
    let qa = q.to_array();
    let mut inverse = [0.0; SHAPES];
    for (slot, shape) in inverse.iter_mut().zip(Shape::ALL) {
        let anchor = shape.anchor().to_array();
        let d2: f64 = (0..DIMS)
            .map(|k| ((qa[k] - anchor[k]) * SCALE[k]).powi(2))
            .sum();
        *slot = 1.0 / d2.sqrt().max(1e-3).powf(POWER);
    }
    let mut sorted = inverse;
    sorted.sort_by(|a, b| b.total_cmp(a));
    let cut = sorted[KEEP - 1];
    let raw = inverse.map(|v| (v - cut).max(0.0));
    let sum: f64 = raw.iter().sum();
    let sum = if sum == 0.0 { 1.0 } else { sum };
    let mut visemes = [0.0; VISEMES];
    for (slot, v) in visemes.iter_mut().zip(&raw[1..]) {
        *slot = v / sum;
    }
    MorphWeights { visemes }
}
