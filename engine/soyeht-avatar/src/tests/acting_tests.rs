use crate::acting::{Acting, ActingFrame, ActingInput, Gaze, Lid, State};
use crate::acting_tune::ActingTune;

const DT: f64 = 1.0 / 60.0;

fn input(state: State, voice: bool) -> ActingInput {
    ActingInput {
        state,
        voice,
        db: voice.then_some(-20.0),
        words_known: false,
        jaw: 0.0,
    }
}

fn run(seed: u64, seconds: f64, state: State, voice: bool) -> Vec<ActingFrame> {
    let mut a = Acting::new(ActingTune::default(), seed, 1.0);
    (0..(seconds / DT) as usize)
        .map(|_| a.step(DT, input(state, voice)))
        .collect()
}

#[test]
fn the_same_seed_gives_the_same_life() {
    let a = run(7, 20.0, State::Idle, false);
    let b = run(7, 20.0, State::Idle, false);
    assert_eq!(a, b);
    assert_ne!(a, run(8, 20.0, State::Idle, false));
}

#[test]
fn an_idle_avatar_blinks_with_a_half_closed_frame_on_each_side() {
    let frames = run(3, 60.0, State::Idle, false);
    let lids: Vec<Lid> = frames.iter().map(|f| f.eye.lid).collect();
    assert!(lids.contains(&Lid::Closed), "no blink in a minute");
    // Every closed lid is entered and left through the half frame.
    for (i, lid) in lids.iter().enumerate().skip(1) {
        if *lid == Lid::Closed && lids[i - 1] != Lid::Closed {
            assert_eq!(lids[i - 1], Lid::Half);
        }
    }
    let gaps: Vec<usize> = lids
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] == Lid::Open && w[1] == Lid::Half)
        .map(|(i, _)| i)
        .collect();
    assert!(gaps.len() >= 6, "too few blinks: {}", gaps.len());
}

#[test]
fn thinking_looks_up_and_away_and_listening_has_soft_eyes() {
    let thinking = run(5, 6.0, State::Thinking, false);
    assert!(
        thinking
            .iter()
            .any(|f| matches!(f.eye.gaze, Gaze::Up | Gaze::UpLeft))
    );
    let listening = run(5, 3.0, State::Listening, false);
    assert!(listening.last().expect("frames").eye.soft);
    assert!(listening.last().expect("frames").pose.cheek > 0.5);
}

#[test]
fn voice_makes_the_avatar_speak_and_silence_ends_it_after_half_a_second() {
    let mut a = Acting::new(ActingTune::default(), 1, 1.0);
    for _ in 0..60 {
        a.step(DT, input(State::Idle, false));
    }
    assert_eq!(a.step(DT, input(State::Idle, true)).state, State::Speaking);
    for _ in 0..20 {
        assert_eq!(a.step(DT, input(State::Idle, false)).state, State::Speaking);
    }
    for _ in 0..30 {
        a.step(DT, input(State::Idle, false));
    }
    assert_eq!(a.state(), State::Idle);
}

#[test]
fn the_pose_stays_small_and_smooth() {
    let frames = run(11, 30.0, State::Speaking, true);
    for pair in frames.windows(2) {
        let (a, b) = (pair[0].pose, pair[1].pose);
        // A nod is a kick of a spring, a few pixels a frame at most; the sway is far slower.
        assert!(
            (a.head_y - b.head_y).abs() < 4.0 && (a.rot - b.rot).abs() < 1.0,
            "a pose jump"
        );
        assert!(b.head_x.abs() < 10.0 && b.head_y.abs() < 10.0 && b.rot.abs() < 10.0);
        assert!((0.0..=1.4).contains(&b.breath));
    }
}

/// How far the brows rise in the next 30 frames of an idle avatar after this word starts (the
/// accents are the same in every state; an idle avatar has the brows down to begin with).
fn lift_after(word: &str, index: usize, count: usize, text: &str) -> f64 {
    let mut a = Acting::new(ActingTune::default(), 2, 1.0);
    for _ in 0..30 {
        a.step(DT, input(State::Idle, false));
    }
    a.word_start(word, index, count, false, text);
    (0..30)
        .map(|_| a.step(DT, input(State::Idle, false)).pose)
        .map(|p| p.brow_l.min(p.brow_r))
        .fold(0.0, f64::max)
}

#[test]
fn a_question_lifts_the_brows_at_its_last_word_and_an_exclamation_accents_its_word() {
    // The default accent is 5 pixels; a question is that and 2 more.
    assert!(lift_after("sério?", 1, 2, "Sério?") > 5.0, "the question");
    assert!(
        lift_after("sim", 0, 2, "Sim sério?") < 0.5,
        "not the last word"
    );
    // An exclamation of two letters or more is accented whatever its length (this one is too short
    // to be a content word); the same word without it, and a lone letter, are not.
    assert!(
        lift_after("pare!", 0, 3, "Pare! Já") > 4.0,
        "the exclamation"
    );
    assert!(lift_after("pare", 0, 3, "Pare já") < 0.5, "no exclamation");
    assert!(lift_after("a!", 0, 3, "A! Já") < 0.5, "a lone letter");
}
