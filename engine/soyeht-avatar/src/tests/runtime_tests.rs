use super::fixtures::{PORTUGUESE, speech};
use crate::acting::{REFERENCE_SIZE, State};
use crate::acting_tune::ActingTune;
use crate::aligner::{Language, align};
use crate::clip::{Clip, WordStart, energy_target};
use crate::envelope::{Envelope, HOP};
use crate::runtime::{Audio, Runtime};
use crate::tune::Tune;

fn runtime(canvas: f64) -> Runtime {
    Runtime::new(Tune::default(), ActingTune::default(), 1, canvas)
}

#[test]
fn the_runtime_follows_the_audio_clock() {
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.05);
    let mut runtime = runtime(1024.0);
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
    let mut runtime = runtime(1024.0);
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

#[test]
fn the_chin_follows_the_mouth_and_lengths_scale_with_the_canvas() {
    // The acting layer is fed the mouth's opening (the mouth steps first), and its lengths are
    // pixels of the canvas: on a canvas half as tall the same performance is half as long.
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.05);
    let play = |canvas: f64| {
        let mut r = runtime(canvas);
        r.set_clip(Some(Clip::new(
            &s.phonemes,
            Some(&s.envelope),
            &Tune::default(),
        )));
        (0..300)
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
            .collect::<Vec<_>>()
    };
    let (full, half) = (play(REFERENCE_SIZE), play(REFERENCE_SIZE / 2.0));
    let widest = full.iter().map(|f| f.0).fold(0.0, f64::max);
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
            "the mouth does not depend on the canvas"
        );
    }
}

#[test]
fn a_clip_with_nothing_to_say_over_a_loud_voice_follows_the_level() {
    let env = Envelope {
        db: vec![-20.0; 100],
        hop: HOP,
    };
    let mut r = runtime(1024.0);
    r.set_clip(Some(Clip::new(&[], Some(&env), &Tune::default())));
    let mut open = 0.0f64;
    for i in 0..120 {
        let t = i as f64 / 60.0;
        open = open.max(
            r.step(
                1.0 / 60.0,
                State::Speaking,
                Some(Audio {
                    position: t,
                    db: Some(-20.0),
                }),
            )
            .mouth
            .o,
        );
    }
    assert!(open > 0.5, "the mouth stayed shut over the voice ({open})");
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
        let mut r = runtime(1024.0);
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
        let mut r = runtime(1024.0);
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
