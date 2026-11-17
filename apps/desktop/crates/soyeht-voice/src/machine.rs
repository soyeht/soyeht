//! The turn-taking rules of the voice loop (ADR 0166 section 5, contract 20 section 2): when the
//! person starts and ends a turn, when the agent's words go out, and when the person talking over
//! the avatar takes the turn. A pure state machine: no clock, no audio, no IO. Time is the stamp
//! of each input, in milliseconds on the microphone's clock, and frames are the only clock.

use crate::config::VoiceTurnConfig;
use crate::interruption::{Continuers, Echo, echo_of};
use crate::text::tokens;

/// Where the machine is: four states in nine phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceTurnPhase {
    /// The loop is not running.
    Off,
    /// Listening, nobody is talking.
    Waiting,
    /// Listening, the person is talking or paused for less than `silence_ms`.
    Hearing,
    /// Listening, the pause is long enough that the end-of-turn model was asked.
    Deciding,
    /// Thinking, the turn ended and the recognizer is producing its final text.
    Finalizing,
    /// Thinking, the person's words went to the agent and nothing is audible yet.
    WaitingReply,
    /// Thinking, someone talks and the machine listens to what they say.
    ProbingWait,
    /// Speaking, the reply is audible.
    Playing,
    /// Speaking, someone has talked over the reply and the machine listens to what they say.
    Probing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceTurnState {
    Idle,
    Listening,
    Thinking,
    Speaking,
}

impl VoiceTurnPhase {
    pub fn state(self) -> VoiceTurnState {
        match self {
            Self::Off => VoiceTurnState::Idle,
            Self::Waiting | Self::Hearing | Self::Deciding => VoiceTurnState::Listening,
            Self::Finalizing | Self::WaitingReply | Self::ProbingWait => VoiceTurnState::Thinking,
            Self::Playing | Self::Probing => VoiceTurnState::Speaking,
        }
    }
}

/// What reaches the machine. Time is not an input of its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VoiceTurnInput<'a> {
    Start,
    Stop,
    /// One voice-activity frame, stamped with the time its audio ended.
    Frame {
        probability: f64,
    },
    /// The end-of-turn model's answer to the check `id`.
    EndOfTurn {
        id: u64,
        complete: bool,
    },
    Transcript {
        text: &'a str,
        is_final: bool,
    },
    /// A sentence of a reply began to play.
    SentenceStarted {
        text: &'a str,
    },
    /// The replies were played to their end, or ended with nothing to say.
    ReplyFinished,
    /// The person asked to stop, with a key or a click.
    Interrupt,
}

/// What the machine asks of the controller. Times are milliseconds on the microphone's clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceTurnAction {
    /// Start a recognizer with the audio from `from_ms` on.
    StartTurn {
        from_ms: i64,
    },
    CheckEndOfTurn {
        id: u64,
    },
    /// Stop feeding the recognizer and wait for its final text.
    FinalizeTurn,
    /// Drop the recognizer and what it heard.
    DiscardTurn,
    /// Send the person's words to the agent; `interrupted` is set when they talked over a reply.
    Submit {
        text: String,
        interrupted: bool,
    },
    CancelReply,
    StopPlayback,
    /// Start a recognizer on what is being said over the reply, or over the wait for it.
    ProbeInterruption {
        from_ms: i64,
    },
    /// The person stopped talking: stop feeding the probe's recognizer and wait for its final text.
    FinishProbe,
    DropProbe,
}

use VoiceTurnAction as Action;
use VoiceTurnPhase as Phase;

pub struct VoiceTurnMachine {
    phase: Phase,
    config: VoiceTurnConfig,
    continuers: Continuers,
    voiced: bool,
    was_in_speech: bool,
    run_start: i64,
    /// The time of the last frame of speech (a voiced run of `speech_start_ms` or more).
    speech_end: i64,
    ignore_until: i64,
    /// The ids of the end-of-turn checks only grow: an answer for an older check is never taken.
    check_id: u64,
    /// The recognizer's text for the turn so far, and its final text when it ended before the turn.
    partial: String,
    final_text: Option<String>,
    /// The words of the recognizers that ended earlier in the same turn.
    carried: String,
    /// When the recognizer of the turn ended: a recognizer that starts after it never starts from
    /// audio the first one already heard.
    ended_at: i64,
    finalize_start: i64,
    interrupted: bool,
    needs_quiet: bool,
    probe_start: i64,
    probe_finishing: bool,
    /// The words of the last two sentences played: what an echo of the speaker would repeat.
    spoken: Vec<Vec<String>>,
}

impl Default for VoiceTurnMachine {
    fn default() -> Self {
        Self::new(VoiceTurnConfig::default())
    }
}

impl VoiceTurnMachine {
    pub fn new(config: VoiceTurnConfig) -> Self {
        Self {
            phase: Phase::Off,
            continuers: Continuers::new(&config.continuers),
            config,
            voiced: false,
            was_in_speech: false,
            run_start: 0,
            speech_end: 0,
            ignore_until: 0,
            check_id: 0,
            partial: String::new(),
            final_text: None,
            carried: String::new(),
            ended_at: 0,
            finalize_start: 0,
            interrupted: false,
            needs_quiet: false,
            probe_start: 0,
            probe_finishing: false,
            spoken: Vec::new(),
        }
    }

    pub fn phase(&self) -> VoiceTurnPhase {
        self.phase
    }

    /// What the machine asks of the controller for one input at time `at`.
    pub fn handle(&mut self, input: VoiceTurnInput<'_>, at: i64) -> Vec<VoiceTurnAction> {
        match input {
            VoiceTurnInput::Start => {
                if self.phase == Phase::Off {
                    self.reset();
                    self.phase = Phase::Waiting;
                }
                Vec::new()
            }
            VoiceTurnInput::Stop => self.stop(),
            VoiceTurnInput::Frame { probability } => self.frame(probability, at),
            VoiceTurnInput::EndOfTurn { id, complete } => self.end_of_turn(id, complete, at),
            VoiceTurnInput::Transcript { text, is_final } => self.transcript(text, is_final, at),
            VoiceTurnInput::SentenceStarted { text } => self.sentence_started(text),
            VoiceTurnInput::ReplyFinished => self.reply_finished(at),
            VoiceTurnInput::Interrupt => self.interrupt(at),
        }
    }

    fn frame(&mut self, probability: f64, at: i64) -> Vec<Action> {
        if at < self.ignore_until {
            self.voiced = false;
            return Vec::new();
        }
        let was = self.voiced;
        self.voiced = probability >= self.config.start_threshold
            || (was && probability >= self.config.end_threshold);
        if self.voiced && !was {
            self.run_start = at - self.config.frame_ms;
        }
        let run = if self.voiced { at - self.run_start } else { 0 };
        let in_speech = self.voiced && run >= self.config.speech_start_ms;
        let resumed = in_speech && !self.was_in_speech;
        self.was_in_speech = in_speech;
        if in_speech {
            self.speech_end = at;
        }
        let silence = if in_speech { 0 } else { at - self.speech_end };
        match self.phase {
            Phase::Off => Vec::new(),
            Phase::Waiting => {
                if !in_speech {
                    return Vec::new();
                }
                self.phase = Phase::Hearing;
                self.begin_turn();
                vec![Action::StartTurn {
                    from_ms: self.restart(at),
                }]
            }
            Phase::Hearing => {
                if in_speech {
                    return if resumed { self.resume(at) } else { Vec::new() };
                }
                if silence < self.config.silence_ms {
                    return Vec::new();
                }
                self.check_id += 1;
                self.phase = Phase::Deciding;
                vec![Action::CheckEndOfTurn { id: self.check_id }]
            }
            Phase::Deciding => {
                if in_speech {
                    self.phase = Phase::Hearing;
                    return self.resume(at);
                }
                if silence >= self.config.incomplete_silence_ms {
                    self.end_turn(at)
                } else {
                    Vec::new()
                }
            }
            Phase::Finalizing => {
                if at - self.finalize_start < self.config.finalize_timeout_ms {
                    return Vec::new();
                }
                let text = self.partial.clone();
                let mut actions = vec![Action::DiscardTurn];
                actions.extend(
                    self.complete(&text)
                        .into_iter()
                        .filter(|action| *action != Action::DiscardTurn),
                );
                actions
            }
            Phase::WaitingReply => {
                if !self.voiced {
                    self.needs_quiet = false;
                }
                if self.talks_over(run) {
                    self.start_probe(Phase::ProbingWait, at)
                } else {
                    Vec::new()
                }
            }
            Phase::Playing => {
                if self.config.half_duplex {
                    return Vec::new();
                }
                if !self.voiced {
                    self.needs_quiet = false;
                }
                if self.talks_over(run) {
                    self.start_probe(Phase::Probing, at)
                } else {
                    Vec::new()
                }
            }
            Phase::Probing | Phase::ProbingWait => {
                if at - self.probe_start >= self.config.probe_timeout_ms {
                    return self.drop_probe(self.voiced);
                }
                if in_speech || silence < self.config.silence_ms || self.probe_finishing {
                    return Vec::new();
                }
                self.probe_finishing = true;
                vec![Action::FinishProbe]
            }
        }
    }

    /// The person has talked, without a break, long enough to be worth listening to.
    fn talks_over(&self, run: i64) -> bool {
        self.voiced && !self.needs_quiet && run >= self.config.barge_in_ms
    }

    /// Where a recognizer started now begins in the audio: before the voice began, by the
    /// pre-roll, never before what is still kept, and never inside the deaf tail of the speaker.
    fn restart(&self, at: i64) -> i64 {
        [
            0,
            self.run_start - self.config.pre_roll_ms,
            at - self.config.history_ms,
            self.ignore_until,
            self.ended_at,
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }

    fn begin_turn(&mut self) {
        self.partial.clear();
        self.final_text = None;
        self.carried.clear();
        self.ended_at = 0;
    }

    /// The person talks again after the recognizer of the turn ended: a recognizer starts for the
    /// new speech, and what the first one heard is kept.
    fn resume(&mut self, at: i64) -> Vec<Action> {
        let Some(ended) = self.final_text.take() else {
            return Vec::new();
        };
        self.carried = joined(&self.carried, &ended);
        self.partial.clear();
        vec![Action::StartTurn {
            from_ms: self.restart(at),
        }]
    }

    fn start_probe(&mut self, to: Phase, at: i64) -> Vec<Action> {
        self.phase = to;
        self.probe_start = at;
        self.probe_finishing = false;
        self.begin_turn();
        vec![Action::ProbeInterruption {
            from_ms: self.restart(at),
        }]
    }

    fn drop_probe(&mut self, still_voiced: bool) -> Vec<Action> {
        self.phase = if self.phase == Phase::Probing {
            Phase::Playing
        } else {
            Phase::WaitingReply
        };
        self.needs_quiet = still_voiced;
        vec![Action::DropProbe]
    }

    /// The turn is over: with the recognizer's text if it already ended, else by asking for it.
    fn end_turn(&mut self, at: i64) -> Vec<Action> {
        if let Some(text) = self.final_text.clone() {
            return self.complete(&text);
        }
        self.phase = Phase::Finalizing;
        self.finalize_start = at;
        vec![Action::FinalizeTurn]
    }

    /// The words of the turn are known: they go to the agent, or the turn was empty.
    fn complete(&mut self, text: &str) -> Vec<Action> {
        let words = joined(&self.carried, text);
        let interrupted = std::mem::take(&mut self.interrupted);
        self.final_text = None;
        self.carried.clear();
        if words.is_empty() {
            self.phase = Phase::Waiting;
            return vec![Action::DiscardTurn];
        }
        self.phase = Phase::WaitingReply;
        vec![Action::Submit {
            text: words,
            interrupted,
        }]
    }

    fn end_of_turn(&mut self, id: u64, complete: bool, at: i64) -> Vec<Action> {
        if self.phase == Phase::Deciding && id == self.check_id && complete {
            self.end_turn(at)
        } else {
            Vec::new()
        }
    }

    fn transcript(&mut self, text: &str, is_final: bool, at: i64) -> Vec<Action> {
        match self.phase {
            Phase::Hearing | Phase::Deciding => {
                self.partial = text.to_owned();
                if is_final {
                    self.ended(text, at)
                } else {
                    Vec::new()
                }
            }
            Phase::Finalizing => {
                self.partial = text.to_owned();
                if is_final {
                    self.complete(text)
                } else {
                    Vec::new()
                }
            }
            Phase::Probing | Phase::ProbingWait => self.probe_transcript(text, is_final, at),
            _ => Vec::new(),
        }
    }

    /// What a recognizer of a probe heard: a listener's noise or an echo drops the probe once the
    /// text is final; anything else takes the turn.
    fn probe_transcript(&mut self, text: &str, is_final: bool, at: i64) -> Vec<Action> {
        let words = tokens(text);
        let heard = if words.is_empty() || self.continuers.contains_only(&words) {
            Echo::Yes
        } else {
            echo_of(&self.spoken, &words)
        };
        match heard {
            Echo::Yes => {
                return if is_final {
                    self.drop_probe(self.voiced)
                } else {
                    Vec::new()
                };
            }
            Echo::Maybe if !is_final => return Vec::new(),
            Echo::Maybe | Echo::No => {}
        }
        let playing = self.phase == Phase::Probing;
        self.phase = Phase::Hearing;
        self.interrupted = playing;
        self.partial = text.to_owned();
        self.final_text = None;
        let mut actions = if playing {
            vec![Action::StopPlayback, Action::CancelReply]
        } else {
            vec![Action::CancelReply]
        };
        if is_final {
            actions.extend(self.ended(text, at));
        }
        actions
    }

    /// The recognizer of the turn ended with `text`. When the person is still talking, a
    /// recognizer starts at once for the rest; otherwise one starts if they speak again before the
    /// turn ends.
    fn ended(&mut self, text: &str, at: i64) -> Vec<Action> {
        self.final_text = Some(text.to_owned());
        self.ended_at = at;
        if self.phase == Phase::Hearing && self.was_in_speech {
            self.resume(at)
        } else {
            Vec::new()
        }
    }

    fn sentence_started(&mut self, text: &str) -> Vec<Action> {
        match self.phase {
            Phase::Waiting | Phase::WaitingReply => {
                self.phase = Phase::Playing;
                self.spoken = vec![tokens(text)];
                self.needs_quiet = false;
                if self.config.half_duplex {
                    self.voiced = false;
                }
            }
            Phase::Playing | Phase::Probing => {
                self.spoken.push(tokens(text));
                let extra = self.spoken.len().saturating_sub(2);
                self.spoken.drain(..extra);
            }
            _ => {}
        }
        Vec::new()
    }

    fn reply_finished(&mut self, at: i64) -> Vec<Action> {
        let actions = match self.phase {
            Phase::WaitingReply => Vec::new(),
            Phase::Playing | Phase::Probing => {
                if self.config.half_duplex {
                    self.voiced = false;
                    self.ignore_until = at + self.config.half_duplex_tail_ms;
                }
                if self.phase == Phase::Probing {
                    vec![Action::DropProbe]
                } else {
                    Vec::new()
                }
            }
            Phase::ProbingWait => vec![Action::DropProbe],
            _ => return Vec::new(),
        };
        self.phase = Phase::Waiting;
        actions
    }

    fn interrupt(&mut self, at: i64) -> Vec<Action> {
        if matches!(self.phase, Phase::Off | Phase::Waiting) {
            return Vec::new();
        }
        let mut actions = self.stopping();
        if self.agent_is_working() {
            actions.push(Action::CancelReply);
        }
        if self.config.half_duplex && matches!(self.phase, Phase::Playing | Phase::Probing) {
            self.ignore_until = at + self.config.half_duplex_tail_ms;
        }
        self.phase = Phase::Waiting;
        self.interrupted = false;
        self.voiced = false;
        actions
    }

    /// Turning the voice off stops playback and listening and leaves the agent to its work.
    fn stop(&mut self) -> Vec<Action> {
        let actions = self.stopping();
        self.reset();
        self.phase = Phase::Off;
        actions
    }

    fn agent_is_working(&self) -> bool {
        matches!(
            self.phase,
            Phase::WaitingReply | Phase::ProbingWait | Phase::Playing | Phase::Probing
        )
    }

    /// What drops what the loop itself has under way in the phase the machine is in.
    fn stopping(&self) -> Vec<Action> {
        match self.phase {
            Phase::Off | Phase::Waiting | Phase::WaitingReply => Vec::new(),
            Phase::Hearing | Phase::Deciding | Phase::Finalizing => vec![Action::DiscardTurn],
            Phase::ProbingWait => vec![Action::DropProbe],
            Phase::Playing => vec![Action::StopPlayback],
            Phase::Probing => vec![Action::DropProbe, Action::StopPlayback],
        }
    }

    fn reset(&mut self) {
        self.voiced = false;
        self.was_in_speech = false;
        self.run_start = 0;
        self.speech_end = 0;
        self.ignore_until = 0;
        self.carried.clear();
        self.ended_at = 0;
        self.partial.clear();
        self.final_text = None;
        self.interrupted = false;
        self.needs_quiet = false;
        self.probe_finishing = false;
        self.spoken.clear();
    }
}

/// The two texts as one, each trimmed, with a blank between them.
fn joined(first: &str, second: &str) -> String {
    [first.trim(), second.trim()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
