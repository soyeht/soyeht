//! The acceptance of ADR 0166 for the mouth: the lab's checker limits, ported (the
//! largest one-frame change at most 0.65, M/B/P closed at least 90 %, silences closed). The ADR
//! adds that a checker pass is not evidence (the rejected H1 mouth passed it), so a person looks at
//! a contact sheet of the whole clip too; these tests are the floor, not the verdict.

use super::checker::{self, Report, talk};
use super::fixtures::{ENGLISH, PORTUGUESE, assets, package_json, speech};
use crate::aligner::Language;
use crate::package::Package;
use crate::params::{Params, Shape};
use crate::tune::Tune;

pub(super) fn package() -> Package {
    Package::load(&package_json().to_string(), &assets()).expect("the synthetic package is valid")
}

fn assert_limits(name: &str, r: &Report) {
    assert!(
        r.pop <= checker::POP,
        "{name}: largest one-frame change {} over {}",
        r.pop,
        checker::POP
    );
    assert!(
        r.pop_closure <= checker::POP_CLOSURE,
        "{name}: largest change in a closure {}",
        r.pop_closure
    );
    assert!(
        r.reversals <= checker::REVERSALS_PER_SECOND,
        "{name}: {} reversals per second",
        r.reversals
    );
    assert!(
        r.correlation >= checker::CORRELATION,
        "{name}: opening against the voice, r = {}",
        r.correlation
    );
    assert!(
        r.mbp_count >= 8,
        "{name}: the speech has too few M, B and P ({})",
        r.mbp_count
    );
    assert!(
        r.mbp >= checker::CLOSED_MBP,
        "{name}: M/B/P closed {}",
        r.mbp
    );
    assert!(
        r.silence_count >= 2,
        "{name}: too few silences ({})",
        r.silence_count
    );
    assert!(
        r.silence >= checker::CLOSED_SILENCE,
        "{name}: silences closed {}",
        r.silence
    );
}

#[test]
fn portuguese_with_the_tts_timings_passes_every_limit() {
    let package = package();
    for gap in [0.0, 0.09] {
        let s = speech(PORTUGUESE, Language::PortugueseBrazil, gap);
        for fps in [checker::LAB_FPS, 30.0, 60.0] {
            let r = talk(
                &package,
                &s.phonemes,
                &s.phonemes,
                &s.envelope,
                fps,
                &Tune::default(),
            );
            assert_limits(&format!("pt gap {gap} at {fps} fps"), &r);
        }
    }
}

#[test]
fn english_with_the_tts_timings_passes_every_limit() {
    let package = package();
    let s = speech(ENGLISH, Language::English, 0.05);
    let r = talk(
        &package,
        &s.phonemes,
        &s.phonemes,
        &s.envelope,
        checker::LAB_FPS,
        &Tune::default(),
    );
    assert_limits("english", &r);
}

#[test]
fn the_checker_fails_a_mouth_that_steps() {
    // A negative control: a mouth with no smoothing and no slew limit changes shape in one frame,
    // and the limit on the largest change catches it.
    let package = package();
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.0);
    let stepping = Tune {
        attack: [0.001; 5],
        release: [0.001; 5],
        max_rate: [1e4; 5],
        weight_smooth: 0.001,
        ..Tune::default()
    };
    let r = talk(
        &package,
        &s.phonemes,
        &s.phonemes,
        &s.envelope,
        checker::LAB_FPS,
        &stepping,
    );
    assert!(
        r.pop > checker::POP,
        "a stepping mouth had a largest change of only {}",
        r.pop
    );
}

#[test]
fn half_way_between_closed_and_a_the_lips_are_half_open_not_two_mouths() {
    let package = package();
    let lips = package
        .mouth
        .parts
        .iter()
        .find(|p| p.id == crate::package::PartKind::Lips)
        .expect("lips part");
    let mut weights = [0.0; 9];
    weights[Shape::ClosedMbp.index()] = 0.5;
    weights[Shape::A.index()] = 0.5;
    let aperture = |w: &[f64; 9]| {
        // The inner contour's lowest point: vertex 2 of the ring of 8, after the 8 outer ones.
        let v = lips.blend(w);
        v[(8 + 2) * 2 + 1] - v[(8 + 6) * 2 + 1]
    };
    let mut closed = [0.0; 9];
    closed[Shape::ClosedMbp.index()] = 1.0;
    let mut open = [0.0; 9];
    open[Shape::A.index()] = 1.0;
    let half = aperture(&weights);
    assert!((half - (aperture(&closed) + aperture(&open)) / 2.0).abs() < 1e-9);
    assert!(half > aperture(&closed) + 1.0 && half < aperture(&open) - 1.0);
}

#[test]
fn every_parameter_stays_in_range_and_the_weights_sum_to_one() {
    let mut mouth = crate::mouth::Mouth::new(Tune::default());
    let targets = [
        Some(Params::new(1.0, 1.0, 1.0, 1.0, 1.0)),
        Some(Params::new(0.0, 0.0, 0.0, 0.0, 0.0)),
        None,
        Some(Params::new(0.7, 0.2, 0.9, 0.0, 0.3)),
    ];
    for step in 0..2000 {
        let q = mouth.step(1.0 / 60.0, targets[(step / 25) % 4]);
        for v in [q.o, q.w, q.r, q.c, q.f] {
            assert!((0.0..=1.0).contains(&v));
        }
        let all = mouth.weights().all();
        assert!(all.iter().all(|w| *w >= 0.0) && (all.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }
}
