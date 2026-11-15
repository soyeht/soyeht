use super::mouth_limits::package;
use crate::params::{Params, Shape, weights_from_params};
use crate::phoneme::{Kind, Phoneme};
use crate::track::{build_track, target_at};
use crate::tune::Tune;

#[test]
fn each_anchor_is_mostly_its_own_drawing() {
    for shape in Shape::ALL {
        let all = weights_from_params(shape.anchor()).all();
        assert!(
            all[shape.index()] > 0.95,
            "{:?} has weight {}",
            shape,
            all[shape.index()]
        );
    }
}

/// The distance to the nearest anchor, in the scaled space the weights use.
fn nearest_anchor(q: Params) -> f64 {
    let scale = [1.3, 1.0, 1.0, 2.0, 2.2];
    Shape::ALL
        .iter()
        .map(|s| {
            let (a, b) = (s.anchor().to_array(), q.to_array());
            (0..5)
                .map(|k| ((a[k] - b[k]) * scale[k]).powi(2))
                .sum::<f64>()
                .sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn the_weights_are_continuous_in_the_parameters() {
    // A small step in the parameter space is a small step in the weights: no drawing switches on
    // abruptly. (Right on an anchor the weights change fast by construction, so the walk skips the
    // neighbourhood of the anchors; a mouth never lands there exactly and the weights are filtered.)
    let mut worst: f64 = 0.0;
    let point = |i: u32| {
        let t = f64::from(i) / 4000.0;
        Params::new(
            t,
            1.0 - t,
            0.5 + 0.5 * (t * 12.0).sin(),
            (1.0 - t) * 0.3,
            t * 0.2,
        )
    };
    for i in 1..=4000 {
        let (before, after) = (point(i - 1), point(i));
        if nearest_anchor(before) < 0.05 || nearest_anchor(after) < 0.05 {
            continue;
        }
        let (a, b) = (
            weights_from_params(before).all(),
            weights_from_params(after).all(),
        );
        worst = worst.max(
            a.iter()
                .zip(&b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0, f64::max),
        );
    }
    assert!(
        worst < 0.05,
        "the largest change of one weight in a 1/4000 step was {worst}"
    );
}

fn ph(p: char, t0: f64, t1: f64) -> Phoneme {
    Phoneme { p, t0, t1 }
}

#[test]
fn a_closure_starts_early_and_is_held_long_enough_to_be_seen() {
    let tune = Tune::default();
    let track = build_track(
        &[ph('a', 0.0, 0.1), ph('m', 0.1, 0.13), ph('a', 0.13, 0.25)],
        &|_, _| 1.0,
        &tune,
    );
    let m = track.iter().find(|t| t.p == 'm').expect("m");
    assert_eq!(m.kind, Kind::Bilabial);
    assert!((m.t0 - (0.1 - tune.closure_lead)).abs() < 1e-12);
    assert!(m.t1 - m.t0 >= tune.min_closure - 1e-12);
    assert_eq!(m.q.c, 1.0);
    // The closure keeps priority over the vowel that starts before it ends.
    assert_eq!(target_at(&track, 0.11).map(|q| q.c), Some(1.0));
}

#[test]
fn a_tongue_consonant_takes_the_shape_of_its_neighbours() {
    let track = build_track(
        &[ph('o', 0.0, 0.12), ph('t', 0.12, 0.2), ph('i', 0.2, 0.32)],
        &|_, _| 1.0,
        &Tune::default(),
    );
    let (o, t, i) = (track[0].q, track[1].q, track[2].q);
    assert_eq!(track[1].kind, Kind::Transparent);
    // The next vowel counts more: the lips are already spreading for i.
    assert!(t.w > o.w && t.w < i.w);
    assert!(t.r < o.r);
}

#[test]
fn a_very_short_unstressed_vowel_is_not_animated() {
    let track = build_track(
        &[ph('a', 0.0, 0.1), ph('e', 0.1, 0.12), ph('a', 0.12, 0.22)],
        &|_, _| 1.0,
        &Tune::default(),
    );
    assert_eq!(track[1].kind, Kind::Transparent);
    let stressed = build_track(
        &[
            ph('a', 0.0, 0.1),
            ph('ˈ', 0.1, 0.1),
            ph('e', 0.1, 0.12),
            ph('a', 0.12, 0.22),
        ],
        &|_, _| 1.0,
        &Tune::default(),
    );
    assert_eq!(stressed[1].kind, Kind::Vowel);
}

#[test]
fn a_loud_stressed_vowel_opens_wider_than_a_soft_plain_one() {
    let tune = Tune::default();
    let soft = build_track(&[ph('e', 0.0, 0.1)], &|_, _| 0.5, &tune)[0].q.o;
    let loud = build_track(&[ph('ˈ', 0.0, 0.0), ph('e', 0.0, 0.1)], &|_, _| 1.2, &tune)[0]
        .q
        .o;
    assert!(loud > soft * 1.5);
}

#[test]
fn nothing_is_said_outside_the_clip() {
    let track = build_track(&[ph('a', 0.2, 0.3)], &|_, _| 1.0, &Tune::default());
    assert_eq!(target_at(&track, 0.0), None);
    assert!(target_at(&track, 0.25).is_some());
    assert_eq!(target_at(&track, 1.0), None);
}

#[test]
fn a_frame_is_one_convex_mix_and_its_picture_an_exact_weighted_average() {
    // The "one mouth" rule, held where it can fail: over a grid of the parameter space, the nine
    // weights are never negative and sum to 1, and the layers of each part of the picture, drawn
    // one over another, give each drawing exactly its weight (raised to the part's texture
    // power) over the sum of the layers'. A blend that took a drawing's alpha wrong would break it.
    let package = package();
    let steps = [0.0, 0.25, 0.5, 0.75, 1.0];
    let mut checked = 0;
    for &o in &steps {
        for &w in &steps {
            for &r in &steps {
                for &c in &steps {
                    for &f in &steps {
                        let all = weights_from_params(Params::new(o, w, r, c, f)).all();
                        assert!(all.iter().all(|x| *x >= 0.0));
                        assert!((all.iter().sum::<f64>() - 1.0).abs() < 1e-9);
                        for part in &package.mouth.parts {
                            let layers = part.layers(&all);
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
                            for (share, power) in shares.iter().zip(&powers) {
                                assert!((share - power / total).abs() < 1e-9);
                            }
                        }
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 3125);
}
