//! Rules of contract 20, section 2, that the sentences of the document state and no case of the
//! vector file pins yet: what a turn keeps of the one before. They are the document's own words.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_voice::{
    VoiceTurnAction as Action, VoiceTurnInput as Input, VoiceTurnMachine, VoiceTurnPhase as Phase,
};

const FRAME_MS: i64 = 32;

/// A machine that is fed frames, one every `FRAME_MS`, and inputs at the time of the last frame.
struct Rig {
    machine: VoiceTurnMachine,
    at: i64,
}

impl Rig {
    fn started() -> Self {
        let mut rig = Self {
            machine: VoiceTurnMachine::default(),
            at: 0,
        };
        rig.input(Input::Start);
        rig
    }

    fn input(&mut self, input: Input<'_>) -> Vec<Action> {
        self.machine.handle(input, self.at)
    }

    /// `count` frames of the given speech probability.
    fn frames(&mut self, count: usize, probability: f64) -> Vec<Action> {
        let mut actions = Vec::new();
        for _ in 0..count {
            self.at += FRAME_MS;
            actions.extend(self.input(Input::Frame { probability }));
        }
        actions
    }

    fn talk(&mut self) -> Vec<Action> {
        self.frames(3, 0.9)
    }

    /// Silence until the end-of-turn model is asked, and its answer: the turn is finished.
    fn pause_and_finish(&mut self) -> Vec<Action> {
        let asked = self.frames(11, 0.1);
        let Some(Action::CheckEndOfTurn { id }) = asked.last() else {
            panic!("the model was not asked: {asked:?}");
        };
        let id = *id;
        self.input(Input::EndOfTurn { id, complete: true })
    }

    fn transcript(&mut self, text: &str, is_final: bool) -> Vec<Action> {
        self.input(Input::Transcript { text, is_final })
    }

    /// A whole turn the recognizer hears as `text`, sent to the agent.
    fn turn(&mut self, text: &str) -> Vec<Action> {
        self.talk();
        self.pause_and_finish();
        self.transcript(text, true)
    }

    /// Silence for longer than the recognizer has to give its final text.
    fn wait_for_the_final_text(&mut self) -> Vec<Action> {
        self.frames(100, 0.1)
    }
}

fn submitted(text: &str, interrupted: bool) -> Action {
    Action::Submit {
        text: text.to_owned(),
        interrupted,
    }
}

#[test]
fn the_last_partial_text_of_the_turn_is_the_final_one_when_the_final_never_comes() {
    let mut rig = Rig::started();
    rig.talk();
    rig.transcript("open the", false);
    rig.pause_and_finish();
    assert_eq!(
        rig.wait_for_the_final_text().last(),
        Some(&submitted("open the", false))
    );
}

#[test]
fn a_turn_that_heard_nothing_is_dropped_whatever_the_turn_before_heard() {
    let mut rig = Rig::started();
    assert_eq!(rig.turn("what time"), [submitted("what time", false)]);
    rig.input(Input::SentenceStarted {
        text: "It is late.",
    });
    rig.input(Input::ReplyFinished);
    rig.talk();
    rig.pause_and_finish();
    assert_eq!(rig.wait_for_the_final_text(), [Action::DiscardTurn]);
    assert_eq!(rig.machine.phase(), Phase::Waiting);
}

#[test]
fn only_the_turn_that_took_the_reply_is_marked_interrupted() {
    let mut rig = Rig::started();
    rig.turn("what time");
    rig.input(Input::SentenceStarted {
        text: "It is half past three.",
    });
    rig.frames(11, 0.9);
    assert_eq!(rig.machine.phase(), Phase::Probing);
    rig.transcript("wait", false);
    rig.pause_and_finish();
    assert_eq!(rig.transcript("wait", true), [submitted("wait", true)]);
    rig.input(Input::ReplyFinished);
    assert_eq!(rig.turn("thanks"), [submitted("thanks", false)]);
}

/// The first recognizer of a turn ended with "open the" and the person spoke again, so a second
/// one runs; it has heard nothing yet.
fn rig_with_a_second_recognizer() -> Rig {
    let mut rig = Rig::started();
    rig.talk();
    rig.frames(3, 0.1);
    rig.transcript("open the", true);
    assert!(matches!(rig.talk().as_slice(), [Action::StartTurn { .. }]));
    rig
}

#[test]
fn the_words_of_the_recognizers_of_a_turn_are_one_turn_without_repeats() {
    let mut rig = rig_with_a_second_recognizer();
    rig.pause_and_finish();
    assert_eq!(
        rig.wait_for_the_final_text().last(),
        Some(&submitted("open the", false))
    );
}

#[test]
fn an_interrupt_drops_the_words_the_turn_had_kept() {
    let mut rig = rig_with_a_second_recognizer();
    assert_eq!(rig.input(Input::Interrupt), [Action::DiscardTurn]);
    assert_eq!(rig.turn("hello"), [submitted("hello", false)]);
}
