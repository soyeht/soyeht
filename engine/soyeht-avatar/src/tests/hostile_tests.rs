//! A package is a file from a stranger and the host's numbers are whatever a machine computes:
//! the crate must bound the first and sanitise the second.

use std::time::Instant;

use serde_json::{Value, json};

use super::fixtures::{PORTUGUESE, assets, package_json, pose_values, speech};
use crate::package::acting_bounds::{BOUNDS, numbers_of};
use crate::package::{ErrorKind, Package};
use crate::params::Params;
use crate::runtime::{Audio, Runtime};
use crate::tune::Tune;
use crate::{
    acting::{Acting, ActingInput, State},
    acting_tune::ActingTune,
    aligner::{Language, align},
    clip::Clip,
    envelope::{Envelope, HOP},
    mouth::Mouth,
};

const DT: f64 = 1.0 / 60.0;

#[test]
fn every_number_of_the_acting_tune_has_a_bound_and_the_defaults_are_inside() {
    let tune = serde_json::to_value(ActingTune::default()).expect("json");
    let mut numbers = Vec::new();
    numbers_of(&tune, "", &mut numbers);
    let named: Vec<&str> = BOUNDS.iter().map(|b| b.0).collect();
    for pointer in &numbers {
        assert!(
            named.contains(&pointer.as_str()),
            "{pointer} has no bound: a package could set it to anything"
        );
    }
    assert_eq!(
        numbers.len(),
        BOUNDS.len(),
        "a bound names a field that does not exist"
    );
    for (pointer, least, most, reason) in BOUNDS {
        let v = tune
            .pointer(pointer)
            .and_then(Value::as_f64)
            .expect("a number");
        assert!(
            (*least..=*most).contains(&v),
            "the default {pointer} = {v} is outside {least}..{most} ({reason})"
        );
    }
}

#[test]
fn a_tune_with_every_constant_at_an_extreme_keeps_the_pose_finite_in_every_state() {
    // Every constant at its most, and every constant at its least, with the gaps and holds at their
    // least too so the layer is busy, through all four states: the pose is finite and small.
    let busy = [
        "/blink/gap/0",
        "/blink/gap/1",
        "/gaze/idleGap/0",
        "/gaze/idleGap/1",
        "/stress/minGap",
        "/stress/wordGap",
    ];
    for extreme in ["most", "least"] {
        let mut tune = serde_json::to_value(ActingTune::default()).expect("json");
        for (pointer, least, most, _) in BOUNDS {
            let value = if extreme == "most" && !busy.contains(pointer) {
                *most
            } else {
                *least
            };
            let slot = tune.pointer_mut(pointer).expect("a field");
            *slot = if slot.is_u64() {
                json!(value as u64)
            } else {
                json!(value)
            };
        }
        let acting: ActingTune = serde_json::from_value(tune).expect("a tune");
        let mut a = Acting::new(acting, 9, 1.0);
        let states = [
            State::Idle,
            State::Listening,
            State::Thinking,
            State::Speaking,
        ];
        let mut worst = 0.0f64;
        for i in 0..4800 {
            let state = states[(i / 150) % 4];
            let voice = state == State::Speaking && i % 120 < 90;
            let f = a.step(
                DT,
                ActingInput {
                    state,
                    voice,
                    db: voice.then_some(-20.0 + (i % 7) as f64 * 4.0),
                    words_known: false,
                    jaw: 4.0,
                },
            );
            if i % 200 == 0 {
                a.word_start("sério?", 1, 2, true, "É sério?");
            }
            for v in pose_values(&f.pose) {
                assert!(v.is_finite(), "{extreme}: frame {i}");
                worst = worst.max(v.abs());
            }
        }
        // Under a seventh of the 1024 pixel picture, however the constants are set.
        assert!(
            worst < 150.0,
            "{extreme}: the largest pose value was {worst}"
        );
    }
}

#[test]
fn a_base_picture_that_is_not_the_canvas_is_refused() {
    let mut doc = package_json();
    doc["files"][0]["width"] = json!(16);
    doc["files"][0]["height"] = json!(16);
    let mut list = assets();
    list[0].size = Some((16, 16));
    let err = Package::load(&doc.to_string(), &list).expect_err("canvas");
    assert_eq!((err.kind, err.at.as_str()), (ErrorKind::Canvas, "/base"));
}

// ------------------------------------------------------------------- inputs from the host

fn finite(q: Params) -> bool {
    q.to_array().iter().all(|v| v.is_finite())
}

#[test]
fn a_mouth_that_is_handed_garbage_stays_finite_and_recovers() {
    let open = Some(Params::new(1.0, 0.5, 0.0, 0.0, 0.0));
    for (name, dt, target) in [
        ("a dt that is not a number", f64::NAN, open),
        ("an infinite dt", f64::INFINITY, open),
        ("a negative dt", -5.0, open),
        ("a dt of an hour", 3600.0, open),
        (
            "a target that is not a number",
            DT,
            Some(Params::new(f64::NAN, 0.5, 0.0, 0.0, 0.0)),
        ),
        (
            "an infinite target",
            DT,
            Some(Params::new(f64::INFINITY, 0.5, 0.0, 0.0, 0.0)),
        ),
    ] {
        let mut m = Mouth::new(Tune::default());
        assert!(finite(m.step(dt, target)), "{name}");
        let mut q = m.params();
        for _ in 0..120 {
            q = m.step(DT, open);
        }
        assert!(
            finite(q) && q.o > 0.9,
            "{name}: the mouth did not recover ({q:?})"
        );
        assert!(m.weights().all().iter().all(|w| w.is_finite()), "{name}");
    }
}

#[test]
fn one_frame_of_minus_infinity_decibels_does_not_end_the_accents() {
    // A silent buffer is -inf dB (log10 of 0). The level that follows must still be an accent.
    let run = |poison: bool| {
        let mut a = Acting::new(ActingTune::default(), 3, 1.0);
        let mut peak = 0.0f64;
        for i in 0..600 {
            let db = if poison && i == 30 {
                f64::NEG_INFINITY
            } else if i < 300 {
                -35.0
            } else {
                -12.0
            };
            let f = a.step(
                DT,
                ActingInput {
                    state: State::Speaking,
                    voice: true,
                    db: Some(db),
                    words_known: false,
                    jaw: 0.0,
                },
            );
            assert!(f.pose.brow_l.is_finite() && f.pose.rot.is_finite());
            if (300..360).contains(&i) {
                peak = peak.max(f.pose.brow_l);
            }
        }
        peak
    };
    let (clean, poisoned) = (run(false), run(true));
    assert!(
        clean > 1.0 && poisoned > 0.5 * clean,
        "clean {clean}, after one -inf frame {poisoned}"
    );
}

#[test]
fn a_runtime_handed_garbage_stays_finite() {
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.05);
    let tune = Tune::default();
    let mut rt = Runtime::with_scale(tune.clone(), ActingTune::default(), 1, 1.0);
    rt.set_clip(Some(Clip::new(&s.phonemes, Some(&s.envelope), &tune)));
    for i in 0..400 {
        let (position, db, dt) = match i {
            50 => (f64::NAN, Some(-20.0), DT),
            60 => (1.0, Some(f64::NAN), DT),
            70 => (1.0, Some(f64::NEG_INFINITY), DT),
            80 => (1.0, Some(f64::INFINITY), f64::NAN),
            90..=120 => (0.2, None, DT),
            _ => (i as f64 / 60.0, Some(-20.0), DT),
        };
        let f = rt.step(dt, State::Speaking, Some(Audio { position, db }));
        assert!(finite(f.mouth), "frame {i}");
        assert!(f.weights.all().iter().all(|w| w.is_finite()), "frame {i}");
        let p = f.acting.pose;
        assert!(
            [p.rot, p.head_x, p.head_y, p.brow_l, p.jaw, p.breath, p.zoom]
                .iter()
                .all(|v| v.is_finite()),
            "frame {i}"
        );
    }
}

#[test]
fn an_envelope_that_is_not_one_gives_no_alignment_and_no_panic() {
    for hop in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e-320] {
        let env = Envelope {
            db: vec![
                -20.0,
                f64::NAN,
                f64::INFINITY,
                -80.0,
                -20.0,
                f64::NEG_INFINITY,
                -20.0,
            ],
            hop,
        };
        let a = align("olá mamãe, tudo bem", Language::PortugueseBrazil, &env);
        assert!(a.phonemes.is_empty(), "hop {hop}");
        let clip = Clip::new(&a.phonemes, Some(&env), &Tune::default());
        assert!(clip.target_at(0.1).is_none());
    }
    // Levels that are not numbers inside a good envelope are the floor.
    let env = Envelope {
        db: vec![
            -20.0,
            f64::NAN,
            f64::INFINITY,
            -80.0,
            -20.0,
            f64::NEG_INFINITY,
            -20.0,
        ],
        hop: HOP,
    };
    assert!(env.reference_db().is_finite());
    assert!(env.gain(env.reference_db(), 0.0, 0.1).is_finite());
}

// --------------------------------------------------------------------------------- cost

#[test]
fn a_word_of_eighty_thousand_consonants_costs_a_pass_not_a_pass_each() {
    let text = "z".repeat(80_000);
    let env = Envelope {
        db: vec![-20.0; 500],
        hop: HOP,
    };
    let t = Instant::now();
    let a = align(&text, Language::English, &env);
    let clip = Clip::new(&a.phonemes, Some(&env), &Tune::default());
    let mut m = Mouth::new(Tune::default());
    for i in 0..600 {
        assert!(finite(m.step(DT, clip.target_at(i as f64 / 60.0))));
    }
    assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
}

#[test]
fn thousands_of_words_and_silences_are_matched_in_time_and_memory_that_grow_with_them() {
    for n in [1_000usize, 8_000] {
        let text = "pa ".repeat(n);
        let mut db = Vec::new();
        for _ in 0..n {
            db.extend([-20.0; 6]);
            db.extend([-80.0; 4]);
        }
        let env = Envelope { db, hop: HOP };
        let t = Instant::now();
        let a = align(&text, Language::PortugueseBrazil, &env);
        assert!(
            a.matched as f64 > 0.9 * a.boundaries as f64,
            "{} of {}",
            a.matched,
            a.boundaries
        );
        assert!(t.elapsed().as_secs() < 5, "{n} words: {:?}", t.elapsed());
    }
}
