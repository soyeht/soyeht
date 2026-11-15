//! The limits of a rendered talk from the lab's `mouth/check_mouth.py`, ported to geometry.
//!
//! The lab measured pixels of a rendered mouth. Here the "picture" is the blended geometry of the
//! mouth mesh, and distances are measured on it, with 1.0 the distance between the closed drawing
//! and the A drawing. The pixel checks (the seam of the morph boundary, a smeared corner) belong
//! to the renderers; the ghost check is the structural form of the same rule: a frame is a
//! convex combination of the nine contours and nothing else, so there is never a second mouth.

use crate::clip::Clip;
use crate::envelope::Envelope;
use crate::mesh::Layer;
use crate::mouth::Mouth;
use crate::package::Package;
use crate::params::{MorphWeights, SHAPES, Shape};
use crate::phoneme::Phoneme;
use crate::tune::Tune;

/// The lab's limits (`LIMITS` of `check_mouth.py`).
pub(super) const GHOST_FRAMES: usize = 0;
pub(super) const POP: f64 = 0.65;
pub(super) const POP_CLOSURE: f64 = 0.70;
pub(super) const REVERSALS_PER_SECOND: f64 = 7.0;
pub(super) const CORRELATION: f64 = 0.55;
pub(super) const CLOSED_MBP: f64 = 0.90;
pub(super) const CLOSED_SILENCE: f64 = 0.90;

/// The lab rendered at 24 fps, and its limits are per frame at that rate.
pub(super) const LAB_FPS: f64 = 24.0;

pub(super) struct Report {
    pub ghost: usize,
    pub pop: f64,
    pub pop_closure: f64,
    pub reversals: f64,
    pub correlation: f64,
    pub mbp: f64,
    pub mbp_count: usize,
    pub silence: f64,
    pub silence_count: usize,
}

fn geometry(package: &Package, weights: &[f64; SHAPES]) -> Vec<f64> {
    package
        .mouth
        .parts
        .iter()
        .flat_map(|p| p.blend(weights))
        .collect()
}

fn pure(shape: Shape) -> [f64; SHAPES] {
    let mut w = [0.0; SHAPES];
    w[Shape::ALL.iter().position(|s| *s == shape).unwrap_or(0)] = 1.0;
    w
}

/// The mean displacement of a vertex between two geometries.
pub(super) fn distance(a: &[f64], b: &[f64]) -> f64 {
    let sum: f64 = a
        .chunks(2)
        .zip(b.chunks(2))
        .map(|(p, q)| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt())
        .sum();
    sum / (a.len() / 2) as f64
}

/// A frame is a ghost when it is not one convex combination of the nine contours, or when its
/// picture layers are not an exact weighted average.
fn is_ghost(weights: &MorphWeights, package: &Package) -> bool {
    let all = weights.all();
    let sum: f64 = all.iter().sum();
    let convex = all.iter().all(|w| *w >= 0.0) && (sum - 1.0).abs() < 1e-9;
    let layers_ok = package.mouth.parts.iter().all(|part| {
        // The layers are drawn over one another; the share of each in the result must be its
        // weight (raised to the part's texture power) over the sum of the layers' weights.
        let layers: Vec<Layer> = part.layers(&all);
        let powers: Vec<f64> = layers
            .iter()
            .map(|l| all[l.shape.index()].powf(part.texture_power))
            .collect();
        let total: f64 = powers.iter().sum();
        let mut remaining = 1.0;
        let mut shares = vec![0.0; layers.len()];
        for (share, layer) in shares.iter_mut().zip(&layers).rev() {
            *share = remaining * layer.alpha;
            remaining *= 1.0 - layer.alpha;
        }
        shares
            .iter()
            .zip(&powers)
            .all(|(s, p)| (s - p / total).abs() < 1e-9)
    });
    !(convex && layers_ok)
}

/// Run the mouth over a clip made from `driven` at `fps` and measure the limits against what
/// is really said (`truth`) and the voice `envelope`. The clip starts after half a second of
/// silence and the run ends 1.6 seconds after the last phoneme.
pub(super) fn talk(
    package: &Package,
    driven: &[Phoneme],
    truth: &[Phoneme],
    envelope: &Envelope,
    fps: f64,
    tune: &Tune,
) -> Report {
    let clip = Clip::new(driven, Some(envelope), tune);
    let lead_in = 0.5;
    let length = truth.last().map_or(0.0, |p| p.t1) + 1.6;
    let frames = ((lead_in + length) * fps) as usize;
    let mut mouth = Mouth::new(tune.clone());
    let mut ghost = 0;
    let mut geoms = Vec::with_capacity(frames);
    for i in 0..frames {
        let t = i as f64 / fps - lead_in;
        let target = if t >= 0.0 {
            clip.target_at(t + tune.lead)
        } else {
            None
        };
        mouth.step(1.0 / fps, target);
        let weights = mouth.weights();
        ghost += usize::from(is_ghost(&weights, package));
        geoms.push(geometry(package, &weights.all()));
    }
    let closed_shape = geometry(package, &pure(Shape::ClosedMbp));
    let rest_shape = geometry(package, &pure(Shape::Rest));
    let scale = distance(&geometry(package, &pure(Shape::A)), &closed_shape);
    let closeness: Vec<f64> = geoms
        .iter()
        .map(|g| distance(g, &closed_shape).min(distance(g, &rest_shape)) / scale)
        .collect();
    let openness: Vec<f64> = closeness.iter().map(|c| c.min(1.2)).collect();
    let closed: Vec<bool> = closeness.iter().map(|c| *c < 0.18).collect();
    let jumps: Vec<f64> = std::iter::once(0.0)
        .chain(geoms.windows(2).map(|w| distance(&w[0], &w[1]) / scale))
        .collect();

    // The voice envelope at the frame times.
    let level_at = |t: f64| -> f64 {
        let rel = t - lead_in;
        if rel < 0.0 || envelope.db.is_empty() {
            return -90.0;
        }
        envelope
            .db
            .get((rel / envelope.hop) as usize)
            .copied()
            .unwrap_or(-90.0)
    };
    let env: Vec<f64> = (0..frames).map(|i| level_at(i as f64 / fps)).collect();

    // A closure and the frames around it (fast, but a few frames) are measured apart.
    let around = (0.17 * fps).round() as usize;
    let in_closure: Vec<bool> = (0..frames)
        .map(|t| {
            closed[t.saturating_sub(around)..(t + around + 1).min(frames)]
                .iter()
                .any(|c| *c)
        })
        .collect();
    let max_of = |wanted: bool| {
        jumps
            .iter()
            .zip(&in_closure)
            .filter(|(_, c)| **c == wanted)
            .map(|(j, _)| *j)
            .fold(0.0, f64::max)
    };

    // The M, B and P phonemes with the lips closed at that moment.
    let (mut mbp_ok, mut mbp_n) = (0, 0);
    for p in truth.iter().filter(|p| super::fixtures::is_closure(p)) {
        let (t0, t1) = (lead_in + p.t0 - 0.09, lead_in + p.t1 + 0.02);
        mbp_n += 1;
        mbp_ok +=
            usize::from((0..frames).any(|i| (t0..=t1).contains(&(i as f64 / fps)) && closed[i]));
    }

    // The silences longer than 150 ms, closed from 150 ms after they start.
    let (mut sil_ok, mut sil_n) = (0, 0);
    let mut i = 0;
    while i < frames {
        if env[i] >= -55.0 {
            i += 1;
            continue;
        }
        let j = (i..frames).find(|k| env[*k] >= -55.0).unwrap_or(frames);
        if (j - i) as f64 / fps > 0.15 && i > 0 {
            sil_n += 1;
            let inside: Vec<usize> = (i..j)
                .filter(|q| {
                    *q as f64 / fps >= i as f64 / fps + 0.15
                        && *q as f64 / fps <= (j - 1) as f64 / fps - 0.06
                })
                .collect();
            sil_ok += usize::from(
                inside.is_empty()
                    || inside.iter().filter(|q| closed[**q]).count() as f64
                        >= 0.85 * inside.len() as f64,
            );
        }
        i = j;
    }

    Report {
        ghost,
        pop: max_of(false),
        pop_closure: max_of(true),
        reversals: reversals(&openness, &env, fps),
        correlation: correlation(&openness, &env, fps),
        mbp: mbp_ok as f64 / mbp_n.max(1) as f64,
        mbp_count: mbp_n,
        silence: sil_ok as f64 / sil_n.max(1) as f64,
        silence_count: sil_n,
    }
}

fn smooth3(v: &[f64]) -> Vec<f64> {
    (0..v.len())
        .map(|i| {
            v[i.saturating_sub(1)..(i + 2).min(v.len())]
                .iter()
                .sum::<f64>()
                / 3.0
        })
        .collect()
}

/// Direction reversals of the opening per second of speech (jitter the eye cannot follow).
fn reversals(openness: &[f64], env: &[f64], fps: f64) -> f64 {
    let smooth = smooth3(openness);
    let signs: Vec<f64> = smooth
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|v| v.abs() > 0.03)
        .map(f64::signum)
        .collect();
    let flips = signs.windows(2).filter(|w| w[0] != w[1]).count();
    let speaking = env.iter().filter(|e| **e > -52.0).count() as f64 / fps;
    flips as f64 / speaking.max(1e-3)
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let (va, vb): (f64, f64) = (
        a.iter().map(|x| (x - ma).powi(2)).sum(),
        b.iter().map(|y| (y - mb).powi(2)).sum(),
    );
    if va < 1e-9 || vb < 1e-9 {
        0.0
    } else {
        cov / (va * vb).sqrt()
    }
}

/// Pearson r between the opening and the voice envelope, at the best lead of 0 to 120 ms.
fn correlation(openness: &[f64], env: &[f64], fps: f64) -> f64 {
    let level: Vec<f64> = smooth3(
        &env.iter()
            .map(|e| ((e + 50.0) / 30.0).clamp(0.0, 1.0))
            .collect::<Vec<_>>(),
    );
    (0..=(0.12 * fps) as usize + 1)
        .map(|lag| pearson(&openness[..openness.len() - lag], &level[lag..]))
        .fold(-1.0, f64::max)
}
