use super::checker::{self, talk};
use super::fixtures::{ENGLISH, PORTUGUESE, is_closure, speech};
use super::mouth_limits::package;
use crate::aligner::{Language, align};
use crate::envelope::{Envelope, HOP};
use crate::phoneme::{Phoneme, STRESS};
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
fn text_and_envelope_alone_drive_a_mouth_that_closes_on_the_lips() {
    // The words-only fallback of the lab closed M/B/P 88 % of the time. The aligner has the
    // silences of the audio too; on this synthetic voice it must pass the lab's limit.
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
        assert_eq!(r.ghost, 0);
        assert!(r.pop <= checker::POP, "pop {}", r.pop);
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
