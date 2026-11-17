//! The turn machine against `contracts/vectors/voice-turns.json`, which the Apple clients pass too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Obj, parse, read_groups};
use serde_json::Value;
use soyeht_voice::{
    VoiceTurnAction, VoiceTurnConfig, VoiceTurnInput, VoiceTurnMachine, VoiceTurnPhase,
};
use std::collections::{BTreeMap, BTreeSet};

const VECTORS: &str = include_str!("../../../../../contracts/vectors/voice-turns.json");

/// The phase names of the vector file.
const PHASES: [(&str, VoiceTurnPhase); 9] = [
    ("off", VoiceTurnPhase::Off),
    ("waiting", VoiceTurnPhase::Waiting),
    ("hearing", VoiceTurnPhase::Hearing),
    ("deciding", VoiceTurnPhase::Deciding),
    ("finalizing", VoiceTurnPhase::Finalizing),
    ("waitingReply", VoiceTurnPhase::WaitingReply),
    ("probingWait", VoiceTurnPhase::ProbingWait),
    ("playing", VoiceTurnPhase::Playing),
    ("probing", VoiceTurnPhase::Probing),
];

/// The kinds of input of the vector file.
const INPUTS: [&str; 8] = [
    "start",
    "stop",
    "frames",
    "endOfTurn",
    "transcript",
    "sentenceStarted",
    "replyFinished",
    "interrupt",
];

/// The numbers of the machine a case may override, in whole milliseconds.
type Setter = fn(&mut VoiceTurnConfig, i64);

const NUMBERS: [(&str, Setter); 5] = [
    ("silenceMs", |config, value| config.silence_ms = value),
    ("incompleteSilenceMs", |config, value| {
        config.incomplete_silence_ms = value;
    }),
    ("finalizeTimeoutMs", |config, value| {
        config.finalize_timeout_ms = value;
    }),
    ("bargeInMs", |config, value| config.barge_in_ms = value),
    ("probeTimeoutMs", |config, value| {
        config.probe_timeout_ms = value;
    }),
];

fn phase_named(name: &str) -> VoiceTurnPhase {
    PHASES
        .iter()
        .find(|(known, _)| *known == name)
        .unwrap_or_else(|| panic!("the vector names a phase that does not exist: {name}"))
        .1
}

fn count(number: i64) -> u64 {
    u64::try_from(number).unwrap()
}

/// The groups of the file: the steps that reach each phase, the cases and the pairs.
struct Vectors<'a> {
    reach: BTreeMap<&'a str, &'a [Value]>,
    cases: Vec<(String, &'a Value)>,
    pairs: Vec<(String, &'a Value)>,
}

impl<'a> Vectors<'a> {
    fn read(file: &'a Value) -> Self {
        read_groups(file, "voice-turns", |groups| {
            let reach: BTreeMap<&str, &[Value]> = groups
                .object("reach")
                .entries()
                .into_iter()
                .map(|(name, steps)| {
                    let steps = steps
                        .as_array()
                        .unwrap_or_else(|| panic!("the reach {name} is not a list"));
                    (name, steps.as_slice())
                })
                .collect();
            let names: BTreeSet<&str> = PHASES.iter().map(|(name, _)| *name).collect();
            assert_eq!(
                reach.keys().copied().collect::<BTreeSet<_>>(),
                names,
                "the file names a way to reach each phase and no other"
            );
            Self {
                reach,
                cases: groups.group("cases"),
                pairs: groups.group("pairs"),
            }
        })
    }
}

/// What an action of the vector file asks, with the time it happened.
fn expected_action(value: &Value, path: &str) -> (VoiceTurnAction, i64) {
    let mut action = Obj::new(value, path);
    let at = action.integer("at");
    let kind = action.string("do");
    let parsed = match kind {
        "startTurn" => VoiceTurnAction::StartTurn {
            from_ms: action.integer("fromMs"),
        },
        "probeInterruption" => VoiceTurnAction::ProbeInterruption {
            from_ms: action.integer("fromMs"),
        },
        "checkEndOfTurn" => VoiceTurnAction::CheckEndOfTurn {
            id: count(action.integer("id")),
        },
        "submit" => VoiceTurnAction::Submit {
            text: action.string("text").to_owned(),
            interrupted: action.boolean("interrupted"),
        },
        "finalizeTurn" => VoiceTurnAction::FinalizeTurn,
        "discardTurn" => VoiceTurnAction::DiscardTurn,
        "cancelReply" => VoiceTurnAction::CancelReply,
        "stopPlayback" => VoiceTurnAction::StopPlayback,
        "finishProbe" => VoiceTurnAction::FinishProbe,
        "dropProbe" => VoiceTurnAction::DropProbe,
        other => panic!("{path} names an action that does not exist: {other}"),
    };
    action.done();
    (parsed, at)
}

/// Gives the machine the inputs one step stands for (a `frames` step is a frame every
/// `frame_ms`), and returns the actions with the time of the input that asked for each.
fn feed(
    machine: &mut VoiceTurnMachine,
    step: &mut Obj<'_>,
    kind: &str,
    at: i64,
    frame_ms: i64,
) -> Vec<(VoiceTurnAction, i64)> {
    let mut inputs: Vec<(VoiceTurnInput<'_>, i64)> = Vec::new();
    match kind {
        "start" => inputs.push((VoiceTurnInput::Start, at)),
        "stop" => inputs.push((VoiceTurnInput::Stop, at)),
        "replyFinished" => inputs.push((VoiceTurnInput::ReplyFinished, at)),
        "interrupt" => inputs.push((VoiceTurnInput::Interrupt, at)),
        "frames" => {
            let frames = step.integer("count");
            let probability = step.number("p");
            for frame in 0..frames {
                let input = VoiceTurnInput::Frame { probability };
                inputs.push((input, at + frame * frame_ms));
            }
        }
        "endOfTurn" => {
            let id = count(step.integer("id"));
            let complete = step.boolean("complete");
            inputs.push((VoiceTurnInput::EndOfTurn { id, complete }, at));
        }
        "transcript" => {
            let text = step.string("text");
            let is_final = step.boolean("final");
            inputs.push((VoiceTurnInput::Transcript { text, is_final }, at));
        }
        "sentenceStarted" => {
            let text = step.string("text");
            inputs.push((VoiceTurnInput::SentenceStarted { text }, at));
        }
        other => panic!("{} is an input that does not exist: {other}", step.path()),
    }
    inputs
        .into_iter()
        .flat_map(|(input, at)| {
            machine
                .handle(input, at)
                .into_iter()
                .map(move |action| (action, at))
        })
        .collect()
}

/// Runs the steps on a machine and checks the actions and the phase after each one. Returns the
/// kind of the last input.
fn run(machine: &mut VoiceTurnMachine, frame_ms: i64, steps: &[Value], path: &str) -> String {
    let mut last = String::new();
    for (index, value) in steps.iter().enumerate() {
        let path = format!("{path}.steps[{index}]");
        let mut step = Obj::new(value, &path);
        let kind = step.string("input");
        let at = step.integer("at");
        let actual = feed(machine, &mut step, kind, at, frame_ms);
        let expected: Vec<(VoiceTurnAction, i64)> = step
            .array("actions")
            .iter()
            .enumerate()
            .map(|(number, action)| expected_action(action, &format!("{path}.actions[{number}]")))
            .collect();
        let phase = step.string("phase");
        step.done();
        assert_eq!(actual, expected, "{path}: actions");
        assert_eq!(machine.phase(), phase_named(phase), "{path}: phase");
        last = kind.to_owned();
    }
    last
}

/// The case's `config`, over the policy's numbers.
fn config_of(case: &mut Obj<'_>) -> VoiceTurnConfig {
    let mut config = VoiceTurnConfig::default();
    let Some(value) = case.optional("config") else {
        return config;
    };
    let mut fields = Obj::new(value, format!("{}.config", case.path()));
    for (key, set) in NUMBERS {
        if fields.has(key) {
            set(&mut config, fields.integer(key));
        }
    }
    if fields.has("halfDuplex") {
        config.half_duplex = fields.boolean("halfDuplex");
    }
    fields.done();
    config
}

/// A case or a pair: a list of steps, after the steps that reach a phase when it names one. Returns
/// the kind of its last input.
fn run_case(vectors: &Vectors<'_>, path: &str, value: &Value) -> String {
    let mut case = Obj::new(value, path);
    case.string("name");
    let config = config_of(&mut case);
    let reach = case.optional("reach").map(|reach| {
        let name = reach
            .as_str()
            .unwrap_or_else(|| panic!("{path}.reach is not a string"));
        let steps = vectors
            .reach
            .get(name)
            .unwrap_or_else(|| panic!("{path} reaches {name}, which the file does not define"));
        (name, *steps)
    });
    let steps = case.array("steps");
    case.done();
    let mut machine = VoiceTurnMachine::new(config.clone());
    if let Some((name, steps)) = reach {
        run(
            &mut machine,
            config.frame_ms,
            steps,
            &format!("reach {name}"),
        );
    }
    run(&mut machine, config.frame_ms, steps, path)
}

#[test]
fn every_case_of_the_vector_file_passes() {
    let file = parse(VECTORS);
    let vectors = Vectors::read(&file);
    for (path, value) in &vectors.cases {
        run_case(&vectors, path, value);
    }
    println!("{} cases", vectors.cases.len());
}

#[test]
fn every_pair_of_the_vector_file_passes_and_no_phase_lacks_an_input() {
    let file = parse(VECTORS);
    let vectors = Vectors::read(&file);
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for (path, value) in &vectors.pairs {
        let reach = Obj::new(value, path.as_str()).string("reach").to_owned();
        seen.insert((reach, run_case(&vectors, path, value)));
    }
    for (phase, _) in PHASES {
        for input in INPUTS {
            assert!(
                seen.contains(&(phase.to_owned(), input.to_owned())),
                "no pair puts {input} to a machine in {phase}"
            );
        }
    }
    println!("{} pairs", vectors.pairs.len());
}
