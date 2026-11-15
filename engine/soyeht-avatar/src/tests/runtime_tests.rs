use super::fixtures::{PORTUGUESE, speech};
use crate::acting::State;
use crate::acting_tune::ActingTune;
use crate::aligner::Language;
use crate::clip::{Clip, energy_target};
use crate::envelope::{Envelope, HOP};
use crate::runtime::{Audio, Runtime};
use crate::tune::Tune;

#[test]
fn the_runtime_follows_the_audio_clock() {
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.05);
    let mut runtime = Runtime::new(Tune::default(), ActingTune::default(), 1);
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
    let mut runtime = Runtime::new(Tune::default(), ActingTune::default(), 1);
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
