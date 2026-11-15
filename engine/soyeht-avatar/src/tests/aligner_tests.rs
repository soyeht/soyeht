use super::checker::{self, talk};
use super::fixtures::{ENGLISH, PORTUGUESE, is_closure, speech};
use super::mouth_limits::package;
use crate::acting::State;
use crate::acting_tune::ActingTune;
use crate::aligner::{Language, align};
use crate::clip::Clip;
use crate::envelope::{Envelope, HOP};
use crate::phoneme::{Phoneme, STRESS};
use crate::runtime::{Audio, Runtime};
use crate::tune::Tune;

fn closure_errors(truth: &[Phoneme], aligned: &[Phoneme]) -> Vec<f64> {
    // The i-th closure of the truth against the i-th of the alignment: the texts are the same, so
    // the phonemes are the same in the same order.
    let mid = |p: &Phoneme| (p.t0 + p.t1) / 2.0;
    truth
        .iter()
        .zip(aligned)
        .filter(|(t, _)| is_closure(t))
        .map(|(t, a)| (mid(t) - mid(a)).abs())
        .collect()
}

#[test]
fn the_aligner_gives_every_sound_of_the_text_a_time_inside_the_voiced_span() {
    for (text, language, gap) in [
        (PORTUGUESE, Language::PortugueseBrazil, 0.09),
        (ENGLISH, Language::English, 0.0),
    ] {
        let s = speech(text, language, gap);
        let a = align(text, language, &s.envelope);
        let (start, end) = a.span.expect("voiced");
        assert_eq!(
            a.phonemes.len(),
            s.phonemes.len(),
            "the same sounds as the truth"
        );
        for (got, want) in a.phonemes.iter().zip(&s.phonemes) {
            assert_eq!(got.p, want.p);
            assert!(
                got.t0 >= start - 1e-9 && got.t1 <= end + 1e-9,
                "{got:?} outside {start}..{end}"
            );
            assert!(got.t1 >= got.t0);
            if got.p == STRESS {
                assert_eq!(got.t0, got.t1);
            }
        }
        for pair in a.phonemes.windows(2) {
            assert!(pair[1].t0 >= pair[0].t0 - 1e-9, "times never go back");
        }
    }
}

#[test]
fn the_silences_of_the_audio_pull_the_word_boundaries_to_where_they_are() {
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.09);
    let a = align(PORTUGUESE, Language::PortugueseBrazil, &s.envelope);
    assert!(a.boundaries >= 15);
    assert!(
        a.matched as f64 >= 0.8 * a.boundaries as f64,
        "matched {} of {}",
        a.matched,
        a.boundaries
    );
    let mut errors = closure_errors(&s.phonemes, &a.phonemes);
    errors.sort_by(f64::total_cmp);
    let median = errors[errors.len() / 2];
    let worst = errors.last().copied().unwrap_or(0.0);
    assert!(median < 0.04, "median closure error {median}");
    assert!(worst < 0.15, "worst closure error {worst}");
}

#[test]
fn text_and_envelope_alone_drive_a_mouth_that_closes_on_the_lips_and_moves_with_the_voice() {
    // The words-only fallback of the lab closed M/B/P 88 % of the time. The aligner has the
    // silences of the audio too; on this synthetic voice it must pass the lab's limits. A track
    // that never moves also closes the lips on every M, B and P, so the opening against the
    // voice is part of the test: it is what a dead mouth fails.
    let package = package();
    for (text, language, gap) in [
        (PORTUGUESE, Language::PortugueseBrazil, 0.09),
        (PORTUGUESE, Language::PortugueseBrazil, 0.0),
        (ENGLISH, Language::English, 0.06),
    ] {
        let s = speech(text, language, gap);
        let a = align(text, language, &s.envelope);
        let r = talk(
            &package,
            &a.phonemes,
            &s.phonemes,
            &s.envelope,
            checker::LAB_FPS,
            &Tune::default(),
        );
        assert!(r.pop <= checker::POP, "pop {}", r.pop);
        assert!(
            r.reversals <= checker::REVERSALS_PER_SECOND,
            "reversals {}",
            r.reversals
        );
        assert!(
            r.correlation >= checker::CORRELATION,
            "gap {gap}: opening against the voice {}",
            r.correlation
        );
        assert!(
            r.mbp >= checker::CLOSED_MBP,
            "gap {gap}: M/B/P closed {}",
            r.mbp
        );
        assert!(
            r.silence >= checker::CLOSED_SILENCE,
            "silences closed {}",
            r.silence
        );
    }
}

#[test]
fn negative_control_a_dead_mouth_passes_the_closures_and_fails_the_voice() {
    // An empty track keeps the lips closed, which is "closed" on every M, B and P and in every
    // silence. Only the opening against the voice sees it.
    let package = package();
    let s = speech(PORTUGUESE, Language::PortugueseBrazil, 0.09);
    let r = talk(
        &package,
        &[],
        &s.phonemes,
        &s.envelope,
        checker::LAB_FPS,
        &Tune::default(),
    );
    assert!(r.mbp >= checker::CLOSED_MBP && r.silence >= checker::CLOSED_SILENCE);
    assert!(
        r.correlation < checker::CORRELATION,
        "a mouth that never opens had r = {}",
        r.correlation
    );
}

fn voice_of_a_sentence() -> Envelope {
    speech(
        "Olá, mamãe bebe pão. Vamos bater um papo sobre a vida.",
        Language::PortugueseBrazil,
        0.09,
    )
    .envelope
}

/// How much of the voice the mouth is open for, and its largest change in one frame.
fn open_while_voiced(clip: Clip, env: &Envelope) -> (f64, f64) {
    let mut runtime = Runtime::with_scale(Tune::default(), ActingTune::default(), 1, 1.0);
    runtime.set_clip(Some(clip));
    let frames = (env.db.len() as f64 * env.hop * 60.0) as usize;
    let (mut open, mut voiced, mut last, mut jump) = (0, 0, 0.0f64, 0.0f64);
    for i in 0..frames {
        let t = i as f64 / 60.0;
        let db = env.db.get((t / env.hop) as usize).copied().unwrap_or(-90.0);
        let f = runtime.step(
            1.0 / 60.0,
            State::Speaking,
            Some(Audio {
                position: t,
                db: Some(db),
            }),
        );
        if db > -40.0 {
            voiced += 1;
            open += usize::from(f.mouth.o > 0.1);
        }
        jump = jump.max((f.mouth.o - last).abs());
        last = f.mouth.o;
    }
    (open as f64 / voiced.max(1) as f64, jump)
}

#[test]
fn text_the_letter_rules_cannot_read_still_gets_a_moving_mouth() {
    let env = voice_of_a_sentence();
    for (text, language) in [
        ("2024", Language::PortugueseBrazil),
        ("3 + 4 = 7", Language::PortugueseBrazil),
        ("São 15:30 de 12/10/2026.", Language::PortugueseBrazil),
        (
            "Custa R$ 1.250,00, 30% a menos.",
            Language::PortugueseBrazil,
        ),
        ("こんにちは、元気ですか", Language::PortugueseBrazil),
        ("Привет, как дела?", Language::English),
    ] {
        let a = align(text, language, &env);
        assert!(
            a.span.is_some() && a.phonemes.len() >= 4,
            "{text:?}: {} phonemes",
            a.phonemes.len()
        );
        let (open, jump) =
            open_while_voiced(Clip::new(&a.phonemes, Some(&env), &Tune::default()), &env);
        assert!(
            open > 0.5,
            "{text:?}: the mouth was open for {open} of the voice"
        );
        assert!(jump < 0.25, "{text:?}: a jump of {jump}");
    }
}

#[test]
fn empty_text_or_a_clip_with_nothing_to_say_falls_back_to_the_level_of_the_voice() {
    let env = voice_of_a_sentence();
    for text in ["", "...  ?!", "()"] {
        let a = align(text, Language::PortugueseBrazil, &env);
        assert!(a.phonemes.is_empty(), "{text:?}");
        let (open, _) =
            open_while_voiced(Clip::new(&a.phonemes, Some(&env), &Tune::default()), &env);
        assert!(
            open > 0.5,
            "{text:?}: the mouth stayed shut over the voice ({open})"
        );
    }
}

#[test]
fn text_much_longer_than_the_audio_is_not_followed_and_the_mouth_follows_the_voice() {
    let env = Envelope {
        db: vec![-20.0; 50],
        hop: HOP,
    };
    let text = "mamãe bebe pão ".repeat(700);
    let a = align(&text, Language::PortugueseBrazil, &env);
    // The text does not belong to this audio: the aligner says so and gives no sounds, and the
    // mouth follows the level of the voice.
    assert!(!a.fits && a.phonemes.is_empty() && a.span.is_some());
    // Text that does belong to its audio fits, fast speech included.
    let spoken = speech(PORTUGUESE, Language::PortugueseBrazil, 0.0);
    assert!(align(PORTUGUESE, Language::PortugueseBrazil, &spoken.envelope).fits);
    let (open, jump) =
        open_while_voiced(Clip::new(&a.phonemes, Some(&env), &Tune::default()), &env);
    assert!(open > 0.5, "open {open}");
    assert!(jump < 0.25, "the slew limit holds: {jump}");
}

#[test]
fn no_voice_or_no_sound_gives_an_empty_track() {
    let quiet = Envelope {
        db: vec![-90.0; 100],
        hop: HOP,
    };
    assert!(
        align("olá", Language::PortugueseBrazil, &quiet)
            .phonemes
            .is_empty()
    );
    let s = speech("olá", Language::PortugueseBrazil, 0.0);
    assert!(
        align("", Language::PortugueseBrazil, &s.envelope)
            .phonemes
            .is_empty()
    );
    assert!(
        align("... ?!", Language::PortugueseBrazil, &s.envelope)
            .phonemes
            .is_empty()
    );
    assert!(
        align(
            "olá",
            Language::PortugueseBrazil,
            &Envelope {
                db: Vec::new(),
                hop: HOP
            }
        )
        .phonemes
        .is_empty()
    );
}

#[test]
fn one_word_fills_the_voiced_span() {
    let s = speech("mamãe", Language::PortugueseBrazil, 0.0);
    let a = align("mamãe", Language::PortugueseBrazil, &s.envelope);
    let (start, end) = a.span.expect("voiced");
    assert!((a.phonemes.first().expect("phonemes").t0 - start).abs() < 1e-9);
    assert!((a.phonemes.last().expect("phonemes").t1 - end).abs() < 1e-9);
    assert_eq!(a.boundaries, 0);
}

#[test]
fn a_pause_mark_takes_the_silence_it_met() {
    let s = speech("sim, não", Language::PortugueseBrazil, 0.0);
    let a = align("sim, não", Language::PortugueseBrazil, &s.envelope);
    let truth = s.phonemes.iter().find(|p| p.p == ',').expect("comma");
    let got = a.phonemes.iter().find(|p| p.p == ',').expect("comma");
    assert!(
        (got.t0 - truth.t0).abs() < 0.05 && (got.t1 - truth.t1).abs() < 0.05,
        "{got:?} against {truth:?}"
    );
}
