use super::fixtures::{PORTUGUESE, speech};
use super::fixtures::{assets, package_json};
use crate::acting::{State, face_scale};
use crate::acting_tune::ActingTune;
use crate::aligner::{Language, align};
use crate::clip::{Clip, WordStart, energy_target};
use crate::envelope::{Envelope, HOP};
use crate::package::Package;
use crate::runtime::{Audio, Runtime};
use crate::tune::Tune;

fn runtime(scale: f64) -> Runtime {
    Runtime::with_scale(Tune::default(), ActingTune::default(), 1, scale)
}

#[test]
fn the_runtime_follows_the_audio_clock() {
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.05);
    let mut runtime = runtime(1.0);
    runtime.set_clip(Some(Clip::new(
        &s.phonemes,
        Some(&s.envelope),
        &Tune::default(),
    )));
    let m = s.phonemes.iter().find(|p| p.p == 'm').expect("an m");
    let mut closed_at_m = false;
    let mut opened = false;
    for i in 0..(s.phonemes.last().map_or(0.0, |p| p.t1) * 60.0) as usize + 120 {
        let t = i as f64 / 60.0;
        let frame = runtime.step(
            1.0 / 60.0,
            State::Speaking,
            Some(Audio {
                position: t,
                db: None,
            }),
        );
        assert_eq!(frame.acting.state, State::Speaking);
        let weights = frame.weights.all();
        assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        closed_at_m |= (m.t0 - 0.05..=m.t1 + 0.1).contains(&t) && frame.mouth.c > 0.6;
        opened |= frame.mouth.o > 0.7;
    }
    assert!(closed_at_m, "the lips never closed on the m");
    assert!(opened, "the mouth never opened");
}

#[test]
fn without_a_clip_the_level_alone_drives_the_opening() {
    assert_eq!(energy_target(-60.0), None);
    let quiet = energy_target(-45.0).expect("voice");
    let loud = energy_target(-15.0).expect("voice");
    assert!(loud.o > quiet.o);
    let mut runtime = runtime(1.0);
    let mut last = 0.0;
    for _ in 0..30 {
        last = runtime
            .step(
                1.0 / 60.0,
                State::Speaking,
                Some(Audio {
                    position: 0.0,
                    db: Some(-15.0),
                }),
            )
            .mouth
            .o;
    }
    assert!(last > 0.3, "opening {last}");
    // Silence closes the lips again.
    for _ in 0..60 {
        last = runtime.step(1.0 / 60.0, State::Idle, None).mouth.o;
    }
    assert!(last < 0.05, "opening {last}");
}

/// A runtime for the synthetic avatar with its head radii scaled by `k`.
fn runtime_for_face(k: f64) -> (Runtime, f64) {
    let mut doc = package_json();
    let radii = doc["warp"]["head"]["radii"]
        .as_array()
        .expect("radii")
        .clone();
    let scaled: Vec<f64> = radii
        .iter()
        .map(|r| r.as_f64().unwrap_or(1.0) * k)
        .collect();
    doc["warp"]["head"]["radii"] = serde_json::json!(scaled);
    let package = Package::load(&doc.to_string(), &assets()).expect("a package");
    let scale = face_scale(package.warp.head.radii);
    (Runtime::new(&package, Tune::default(), 1), scale)
}

#[test]
fn the_chin_follows_the_mouth_and_lengths_scale_with_the_face_the_package_carries() {
    // The acting layer is fed the mouth's opening (the mouth steps first), and its lengths are
    // pixels of the face: with the head half as large, the same performance is half as long,
    // whatever the canvas around it.
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.05);
    let play = |k: f64| {
        let (mut r, scale) = runtime_for_face(k);
        r.set_clip(Some(Clip::new(
            &s.phonemes,
            Some(&s.envelope),
            &Tune::default(),
        )));
        let frames: Vec<(f64, f64, f64)> = (0..300)
            .map(|i| {
                let f = r.step(
                    1.0 / 60.0,
                    State::Speaking,
                    Some(Audio {
                        position: i as f64 / 60.0,
                        db: None,
                    }),
                );
                (f.acting.pose.jaw, f.acting.pose.head_y, f.mouth.o)
            })
            .collect();
        (frames, scale)
    };
    let ((full, scale), (half, half_scale)) = (play(1.0), play(0.5));
    assert!((half_scale * 2.0 - scale).abs() < 1e-12);
    // In the pixels of the reference face the chin drops by up to the lab's 4.6.
    let widest = full.iter().map(|f| f.0).fold(0.0, f64::max) / scale;
    assert!(
        widest > 2.0 && widest < 4.7,
        "the chin dropped {widest} pixels at the most"
    );
    // The chin drops when the mouth opens, not before: with the mouth shut it is at rest.
    assert!(full[0].0 < 0.1);
    for (f, h) in full.iter().zip(&half) {
        assert!((f.0 * 0.5 - h.0).abs() < 1e-9 && (f.1 * 0.5 - h.1).abs() < 1e-9);
        assert!(
            (f.2 - h.2).abs() < 1e-12,
            "the mouth does not depend on the face"
        );
    }
}

#[test]
fn leaving_an_expression_goes_to_rest_and_not_through_pressed_lips() {
    // An expression is not speech: it does not restart the silence rule. After a long listening
    // the lips go back to rest, not to the closed drawing of a pause; and the thinking mouth is
    // left without passing over its own closure.
    let tune = ActingTune::default();
    for (from, held) in [
        (State::Listening, 0.0),
        (State::Thinking, tune.expression.thinking.c),
    ] {
        let mut r = runtime(1.0);
        for _ in 0..240 {
            r.step(1.0 / 60.0, from, None);
        }
        let mut worst = 0.0f64;
        for _ in 0..120 {
            let f = r.step(1.0 / 60.0, State::Idle, None);
            worst = worst.max(f.mouth.c);
            assert!((f.weights.all().iter().sum::<f64>() - 1.0).abs() < 1e-9);
        }
        assert!(
            worst <= held + 0.05,
            "leaving {from:?}: the lips closed to {worst}"
        );
    }
    // Speech that has just ended still closes the lips, as a pause does.
    let mut r = runtime(1.0);
    for i in 0..60 {
        r.step(
            1.0 / 60.0,
            State::Speaking,
            Some(Audio {
                position: i as f64 / 60.0,
                db: Some(-20.0),
            }),
        );
    }
    let mut closed = 0.0f64;
    for _ in 0..30 {
        closed = closed.max(r.step(1.0 / 60.0, State::Idle, None).mouth.c);
    }
    assert!(
        closed > 0.5,
        "the lips did not close after speech ({closed})"
    );
}

#[test]
fn timings_a_tts_gives_are_followed_at_any_speed() {
    // The lab's real clips have 13 to 14 sounds a second, and 34 at two and a half times their
    // speed. The synthetic speech has about 8, so 4.5 times its speed is the same density. Timings
    // a TTS gives are what the voice says at any speed: the clip follows them, with no level to
    // help.
    const SPEEDUP: f64 = 4.5;
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.0);
    let sounds = s.phonemes.iter().filter(|p| p.t1 > p.t0).count() as f64;
    let length = s.phonemes.last().map_or(1.0, |p| p.t1);
    let compressed: Vec<crate::phoneme::Phoneme> = s
        .phonemes
        .iter()
        .map(|p| crate::phoneme::Phoneme {
            p: p.p,
            t0: p.t0 / SPEEDUP,
            t1: p.t1 / SPEEDUP,
        })
        .collect();
    assert!(
        sounds / (length / SPEEDUP) > 30.0,
        "the compressed speech has {} sounds a second",
        sounds / (length / SPEEDUP)
    );
    let mut r = runtime(1.0);
    r.set_clip(Some(Clip::new(&compressed, None, &Tune::default())));
    let frames = (length / SPEEDUP * 60.0) as usize + 30;
    let (mut open, mut closed) = (0, 0);
    for i in 0..frames {
        let f = r.step(
            1.0 / 60.0,
            State::Speaking,
            Some(Audio {
                position: i as f64 / 60.0,
                db: None,
            }),
        );
        open += usize::from(f.mouth.o > 0.2);
        closed += usize::from(f.mouth.c > 0.5);
    }
    assert!(
        open as f64 > 0.2 * frames as f64,
        "open in {open} of {frames} frames"
    );
    assert!(closed > 0, "no closure at all");
}

#[test]
fn the_runtime_raises_the_accent_of_a_word_the_audio_reaches() {
    let words = vec![WordStart {
        t: 0.5,
        word: "sério?".to_string(),
        prev_ends_clause: false,
    }];
    let clip = || Clip::new(&[], None, &Tune::default()).with_words("Sério?", words.clone());
    let play = |with_words: bool| {
        let mut r = runtime(1.0);
        r.set_clip(Some(if with_words {
            clip()
        } else {
            Clip::new(&[], None, &Tune::default())
        }));
        let mut brow = 0.0f64;
        for i in 0..150 {
            let f = r.step(
                1.0 / 60.0,
                State::Speaking,
                Some(Audio {
                    position: i as f64 / 60.0,
                    db: None,
                }),
            );
            if (60..100).contains(&i) {
                brow = brow.max(f.acting.pose.brow_l.min(f.acting.pose.brow_r));
            }
        }
        brow
    };
    // A question lifts both brows at its last word, from the clip's own word list.
    assert!(play(true) > 1.0, "the brows did not lift");
    assert!(play(false) < 0.5);
    // The alignment of a text carries its word starts.
    let s = speech("Olá, mamãe bebe pão.", Language::PortugueseBrazil, 0.09);
    let a = align(
        "Olá, mamãe bebe pão.",
        Language::PortugueseBrazil,
        &s.envelope,
    );
    assert_eq!(a.words.len(), 4);
    assert!(a.words.windows(2).all(|w| w[0].t < w[1].t));
    assert!(a.words[1].prev_ends_clause && !a.words[2].prev_ends_clause);
}

#[test]
fn a_listening_or_thinking_avatar_holds_an_expression_with_the_mouth_it_has() {
    // The resting expressions are targets of the continuous mouth: no picture of a mouth is
    // swapped in, and the weights still sum to 1.
    let tune = ActingTune::default();
    let hold = |state: State| {
        let mut r = runtime(1.0);
        let mut last = r.step(1.0 / 60.0, state, None);
        for _ in 0..120 {
            last = r.step(1.0 / 60.0, state, None);
            assert!((last.weights.all().iter().sum::<f64>() - 1.0).abs() < 1e-9);
        }
        last.mouth
    };
    let (listening, thinking, idle) = (
        hold(State::Listening),
        hold(State::Thinking),
        hold(State::Idle),
    );
    assert!((listening.w - tune.expression.listening.w).abs() < 0.03 && listening.w > idle.w + 0.1);
    assert!((thinking.c - tune.expression.thinking.c).abs() < 0.03 && thinking.c > 0.4);
    assert!(idle.c < 0.05);
}

#[test]
fn the_envelope_of_pcm_has_a_value_per_hop_and_reads_the_level() {
    let rate = 16_000u32;
    let tone: Vec<f32> = (0..rate as usize)
        .map(|i| {
            if i < 8_000 {
                0.5 * (i as f32 * 0.3).sin()
            } else {
                0.0
            }
        })
        .collect();
    let env = Envelope::from_pcm(&tone, rate, HOP);
    assert_eq!(env.db.len(), 50);
    assert!(env.db[10] > -12.0 && env.db[10] < -6.0, "{}", env.db[10]);
    assert!(env.db[40] < -90.0);
    assert!(env.reference_db() > -12.0);
    // A window past the end of the clip has no voice: the gain is the lowest.
    assert_eq!(env.gain(env.reference_db(), 5.0, 5.1), 0.5);
}
