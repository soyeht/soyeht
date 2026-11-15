//! The vectors of the mouth, the aligner and the acting layer: `avatar-mouth.json`.

use serde_json::{Value, json};

use super::{FPS, TOLERANCE, check_group, numbers, read, render, round, updating, vectors_dir};
use crate::acting::{Acting, ActingInput, Gaze, Lid, State};
use crate::acting_tune::ActingTune;
use crate::aligner::{Language, align};
use crate::clip::Clip;
use crate::envelope::Envelope;
use crate::mouth::Mouth;
use crate::params::{Params, Shape, weights_from_params};
use crate::phoneme::Phoneme;
use crate::tests::fixtures::{ENGLISH, PORTUGUESE, speech};
use crate::tune::Tune;

fn params_of(v: &Value) -> Params {
    let n = |k: &str| v[k].as_f64().unwrap_or(0.0);
    Params::new(n("o"), n("w"), n("r"), n("c"), n("f"))
}

fn phonemes_of(v: &Value) -> Vec<Phoneme> {
    let list = v.as_array().map_or(&[][..], Vec::as_slice);
    list.iter()
        .map(|p| Phoneme {
            p: p["p"]
                .as_str()
                .and_then(|s| s.chars().next())
                .unwrap_or(' '),
            t0: p["t0"].as_f64().unwrap_or(0.0),
            t1: p["t1"].as_f64().unwrap_or(0.0),
        })
        .collect()
}

fn phoneme_values(list: &[Phoneme]) -> Value {
    Value::Array(
        list.iter()
            .map(|p| json!({ "p": p.p.to_string(), "t0": round(p.t0), "t1": round(p.t1) }))
            .collect(),
    )
}

fn envelope_of(v: &Value) -> Option<Envelope> {
    let hop = v["hop"].as_f64()?;
    let db = v["db"]
        .as_array()?
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    Some(Envelope { db, hop })
}

fn language_of(id: &str) -> Language {
    if id == "en" {
        Language::English
    } else {
        Language::PortugueseBrazil
    }
}

// ------------------------------------------------------------------------------ mouth cases

fn run_weights(case: &Value) -> Value {
    json!({ "weights": numbers(&weights_from_params(params_of(&case["params"])).all()) })
}

fn run_dynamics(case: &Value) -> Value {
    let tune = Tune::default();
    let envelope = envelope_of(&case["envelope"]);
    let clip = Clip::new(&phonemes_of(&case["phonemes"]), envelope.as_ref(), &tune);
    let track: Vec<Value> = clip
        .track()
        .iter()
        .map(|t| json!({ "p": t.p.to_string(), "kind": t.kind, "t0": round(t.t0), "t1": round(t.t1), "q": numbers(&t.q.to_array()) }))
        .collect();
    let frames = case["frames"].as_u64().unwrap_or(0) as usize;
    let mut mouth = Mouth::new(tune.clone());
    let samples: Vec<Value> = (0..frames)
        .map(|i| {
            let q = mouth.step(1.0 / FPS, clip.target_at(i as f64 / FPS + tune.lead));
            json!({ "params": numbers(&q.to_array()), "weights": numbers(&mouth.weights().all()) })
        })
        .collect();
    json!({ "track": track, "samples": samples })
}

fn run_aligner(case: &Value) -> Value {
    let Some(envelope) = envelope_of(&case["envelope"]) else {
        return Value::Null;
    };
    let a = align(
        case["text"].as_str().unwrap_or(""),
        language_of(case["language"].as_str().unwrap_or("")),
        &envelope,
    );
    let span = a.span.map_or(Value::Null, |(s, e)| numbers(&[s, e]));
    json!({ "span": span, "matched": a.matched, "boundaries": a.boundaries, "phonemes": phoneme_values(&a.phonemes) })
}

fn state_of(id: &str) -> State {
    match id {
        "listening" => State::Listening,
        "thinking" => State::Thinking,
        "speaking" => State::Speaking,
        _ => State::Idle,
    }
}

fn gaze_id(g: Gaze) -> &'static str {
    match g {
        Gaze::Center => "center",
        Gaze::Left => "left",
        Gaze::Right => "right",
        Gaze::Up => "up",
        Gaze::Down => "down",
        Gaze::UpLeft => "upleft",
    }
}

fn lid_id(l: Lid) -> &'static str {
    match l {
        Lid::Open => "open",
        Lid::Half => "half",
        Lid::Closed => "closed",
    }
}

/// `scenes`: `[{state, voice, db, seconds}]`, stepped at 60 fps; the pose is sampled every
/// `sampleEvery` frames.
fn run_acting(case: &Value) -> Value {
    let mut acting = Acting::new(ActingTune::default(), case["seed"].as_u64().unwrap_or(0));
    let every = case["sampleEvery"].as_u64().unwrap_or(30) as usize;
    let mut samples = Vec::new();
    let mut frame = 0usize;
    for scene in case["scenes"].as_array().map_or(&[][..], Vec::as_slice) {
        let voice = scene["voice"].as_bool().unwrap_or(false);
        let input = ActingInput {
            state: state_of(scene["state"].as_str().unwrap_or("")),
            voice,
            db: voice.then(|| scene["db"].as_f64().unwrap_or(-20.0)),
            words_known: false,
            jaw: 0.0,
        };
        for _ in 0..(scene["seconds"].as_f64().unwrap_or(0.0) * FPS).round() as usize {
            let f = acting.step(1.0 / FPS, input);
            frame += 1;
            if frame.is_multiple_of(every) {
                let p = f.pose;
                let pose = [
                    p.rot,
                    p.head_x,
                    p.head_y,
                    p.face_x,
                    p.face_y,
                    p.brow_l,
                    p.brow_r,
                    p.brow_tilt,
                    p.jaw,
                    p.cheek,
                    p.breath,
                    p.zoom,
                ];
                samples.push(json!({ "pose": numbers(&pose), "gaze": gaze_id(f.eye.gaze), "lid": lid_id(f.eye.lid), "soft": f.eye.soft }));
            }
        }
    }
    json!({ "samples": samples })
}

// -------------------------------------------------------------------------- building the file

fn dynamics_case(name: &str, text: &str, language: Language, gap: f64, seconds: f64) -> Value {
    let s = speech(text, language, gap);
    // The speech starts a little after the clip's time 0, as it does in a real clip.
    let shifted: Vec<Phoneme> = s
        .phonemes
        .iter()
        .map(|p| Phoneme {
            p: p.p,
            t0: p.t0 + 0.1,
            t1: p.t1 + 0.1,
        })
        .collect();
    let mut db = vec![-80.0; 5];
    db.extend(&s.envelope.db);
    let mut case = json!({
        "name": name,
        "phonemes": phoneme_values(&shifted),
        "envelope": { "hop": s.envelope.hop, "db": db },
        "frames": (seconds * FPS).round() as u64,
    });
    case["expected"] = run_dynamics(&case);
    case
}

fn aligner_case(name: &str, text: &str, language: &str, gap: f64) -> Value {
    let s = speech(text, language_of(language), gap);
    let mut case = json!({ "name": name, "text": text, "language": language, "envelope": { "hop": s.envelope.hop, "db": s.envelope.db } });
    case["expected"] = run_aligner(&case);
    case
}

fn mouth_vectors() -> Value {
    let weights: Vec<Value> = [
        ("rest", Params::REST),
        ("each anchor: A", Shape::A.anchor()),
        ("each anchor: closed", Shape::ClosedMbp.anchor()),
        ("each anchor: U", Shape::U.anchor()),
        ("between A and E", Params::new(0.7, 0.72, 0.05, 0.0, 0.0)),
        (
            "closing while opening",
            Params::new(0.5, 0.5, 0.2, 0.5, 0.0),
        ),
        ("rounded and spread", Params::new(0.4, 0.5, 0.5, 0.0, 0.5)),
        ("teeth on the lip", Params::new(0.15, 0.7, 0.0, 0.0, 0.9)),
    ]
    .iter()
    .map(|(name, q)| {
        let mut case =
            json!({ "name": name, "params": { "o": q.o, "w": q.w, "r": q.r, "c": q.c, "f": q.f } });
        case["expected"] = run_weights(&case);
        case
    })
    .collect();
    let dynamics = vec![
        dynamics_case(
            "portuguese with closures",
            "Olá, mamãe bebe pão.",
            Language::PortugueseBrazil,
            0.09,
            3.4,
        ),
        dynamics_case(
            "portuguese words run together",
            "Bom dia, vamos papear.",
            Language::PortugueseBrazil,
            0.0,
            3.2,
        ),
        dynamics_case(
            "english",
            "Maybe tomorrow, please.",
            Language::English,
            0.05,
            3.0,
        ),
    ];
    let aligner = vec![
        aligner_case(
            "portuguese with gaps between words",
            "Olá, mamãe bebe pão.",
            "pt-BR",
            0.09,
        ),
        aligner_case(
            "portuguese words run together",
            "Bom dia, vamos papear.",
            "pt-BR",
            0.0,
        ),
        aligner_case("english", "Maybe tomorrow, please.", "en", 0.05),
        aligner_case("a long sentence", PORTUGUESE, "pt-BR", 0.09),
        aligner_case("english sentences", ENGLISH, "en", 0.0),
    ];
    let scenes = json!([
        { "state": "idle", "voice": false, "seconds": 3.0 },
        { "state": "listening", "voice": false, "seconds": 3.0 },
        { "state": "thinking", "voice": false, "seconds": 3.0 },
        { "state": "speaking", "voice": true, "db": -22.0, "seconds": 4.0 },
        { "state": "idle", "voice": false, "seconds": 3.0 },
    ]);
    let acting: Vec<Value> = [7u64, 12345]
        .iter()
        .map(|seed| {
            let mut case = json!({ "name": format!("a day in the life, seed {seed}"), "seed": seed, "sampleEvery": 30, "scenes": scenes });
            case["expected"] = run_acting(&case);
            case
        })
        .collect();
    json!({
        "description": "The mouth runtime of soyeht-avatar (ADR 0166). weights: the nine morph weights (Shape order: rest, closed_MBP, A, E, I, O, U, FV, LDT) of five parameters. dynamics: a clip's phonemes and voice envelope in, the target track out, then the five parameters and the nine weights at each 1/60 s; the target at frame i is the track's target at i/60 + 0.045 (the lead), None is silence, and the mouth steps by 1/60 s. aligner: a text, its language and the envelope of the audio in, the timed phonemes out. acting: a seed (SplitMix64, `unit` is the top 53 bits over 2^53) and scenes in, the pose of the head and the eye frame every 30 frames out. Parameters and weights agree within `tolerance`; the rest agree exactly.",
        "tolerance": TOLERANCE,
        "fps": FPS,
        "weights": weights,
        "dynamics": dynamics,
        "aligner": aligner,
        "acting": acting,
    })
}

#[test]
fn vectors_mouth_file_matches_the_runtime() {
    if updating() {
        std::fs::write(
            vectors_dir().join("avatar-mouth.json"),
            render(&mouth_vectors()),
        )
        .expect("write");
    }
    let file = read("avatar-mouth.json");
    assert_eq!(file["tolerance"].as_f64(), Some(TOLERANCE));
    check_group(&file, "weights", run_weights);
    check_group(&file, "dynamics", run_dynamics);
    check_group(&file, "aligner", run_aligner);
    check_group(&file, "acting", run_acting);
    // A guard on the file itself: an aligner case whose envelope is silent would pass for nothing.
    for case in file["aligner"].as_array().expect("aligner") {
        assert!(
            case["expected"]["phonemes"]
                .as_array()
                .is_some_and(|p| p.len() > 5),
            "{}",
            case["name"]
        );
        assert!(
            case["expected"]["matched"].as_u64().unwrap_or(0) >= 1,
            "{}",
            case["name"]
        );
    }
}
