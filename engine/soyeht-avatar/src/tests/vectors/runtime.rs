//! The vectors of the runtime (the mouth, the acting layer and the clip together) and of the warp,
//! both in `avatar-mouth.json`.

use serde_json::{Value, json};

use super::mouth::{envelope_of, phoneme_values, phonemes_of, state_id, state_of};
use super::{FPS, numbers, round};
use crate::acting::{Pose, REFERENCE_SIZE};
use crate::acting_tune::ActingTune;
use crate::aligner::{Language, align};
use crate::clip::{Clip, WordStart};
use crate::package::Warp;
use crate::runtime::{Audio, Runtime};
use crate::tests::fixtures::{package_json, speech};
use crate::tune::Tune;

/// Samples are every this many frames.
const EVERY: usize = 6;

/// `phonemes`, `envelope`, `text` and `words` in; then `script` (`state`, `audio`, `seconds`) at 60
/// fps. The audio's position is the time since its first frame, and its level is the envelope's at
/// `floor(position / hop)` (-90 past the end). Out: the mouth's five parameters, the jaw of the
/// pose and the first brow, and the state, every 6 frames.
pub(super) fn run_runtime(case: &Value) -> Value {
    let tune = Tune::default();
    let envelope = envelope_of(&case["envelope"]);
    let mut clip = Clip::new(&phonemes_of(&case["phonemes"]), envelope.as_ref(), &tune);
    let words: Vec<WordStart> = case["words"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|w| WordStart {
            t: w["t"].as_f64().unwrap_or(0.0),
            word: w["word"].as_str().unwrap_or("").to_string(),
            prev_ends_clause: w["prevEndsClause"].as_bool().unwrap_or(false),
        })
        .collect();
    if !words.is_empty() {
        clip = clip.with_words(case["text"].as_str().unwrap_or(""), words);
    }
    let mut runtime = Runtime::new(
        tune,
        ActingTune::default(),
        case["seed"].as_u64().unwrap_or(0),
        case["canvasHeight"].as_f64().unwrap_or(1024.0),
    );
    runtime.set_clip(Some(clip));
    let (mut frame, mut audio_start) = (0usize, None);
    let mut samples = Vec::new();
    for scene in case["script"].as_array().map_or(&[][..], Vec::as_slice) {
        let state = state_of(scene["state"].as_str().unwrap_or(""));
        for _ in 0..(scene["seconds"].as_f64().unwrap_or(0.0) * FPS).round() as usize {
            let audio = scene["audio"].as_bool().unwrap_or(false).then(|| {
                let start = *audio_start.get_or_insert(frame);
                let position = (frame - start) as f64 / FPS;
                let db = envelope.as_ref().map_or(-90.0, |e| {
                    e.db.get((position / e.hop) as usize)
                        .copied()
                        .unwrap_or(-90.0)
                });
                Audio {
                    position,
                    db: Some(db),
                }
            });
            let f = runtime.step(1.0 / FPS, state, audio);
            frame += 1;
            if frame.is_multiple_of(EVERY) {
                let q = f.mouth;
                samples.push(json!({
                    "mouth": numbers(&[q.o, q.w, q.r, q.c, q.f]),
                    "jaw": round(f.acting.pose.jaw),
                    "brow": round(f.acting.pose.brow_l),
                    "state": state_id(f.acting.state),
                }));
            }
        }
    }
    json!({ "samples": samples })
}

fn runtime_case(
    name: &str,
    text: &str,
    language: Language,
    canvas_height: f64,
    with_clip: bool,
) -> Value {
    let s = speech(text, language, 0.09);
    let aligned = align(text, language, &s.envelope);
    let words: Vec<Value> = aligned
        .words
        .iter()
        .map(|w| json!({ "t": round(w.t), "word": w.word, "prevEndsClause": w.prev_ends_clause }))
        .collect();
    let phonemes = if with_clip {
        phoneme_values(&aligned.phonemes)
    } else {
        json!([])
    };
    let length = s.envelope.db.len() as f64 * s.envelope.hop;
    let mut case = json!({
        "name": name,
        "text": text,
        "seed": 5,
        "canvasHeight": canvas_height,
        "phonemes": phonemes,
        "words": if with_clip { words } else { vec![] },
        "envelope": { "hop": s.envelope.hop, "db": s.envelope.db },
        "script": [
            { "state": "listening", "audio": false, "seconds": 1.5 },
            { "state": "thinking", "audio": false, "seconds": 1.5 },
            { "state": "speaking", "audio": true, "seconds": length + 0.5 },
            { "state": "idle", "audio": false, "seconds": 1.0 },
        ],
    });
    case["expected"] = run_runtime(&case);
    case
}

pub(super) fn runtime_cases() -> Vec<Value> {
    vec![
        runtime_case(
            "a clip with its words",
            "Olá, mamãe bebe pão.",
            Language::PortugueseBrazil,
            1024.0,
            true,
        ),
        runtime_case(
            "the same on a canvas half as tall",
            "Olá, mamãe bebe pão.",
            Language::PortugueseBrazil,
            REFERENCE_SIZE / 2.0,
            true,
        ),
        runtime_case(
            "a clip with nothing to say over a loud voice",
            "Bom dia, vamos papear.",
            Language::PortugueseBrazil,
            1024.0,
            false,
        ),
    ]
}

// ------------------------------------------------------------------------------------ warp

fn pose_of(v: &Value) -> Pose {
    let n = |k: &str| v[k].as_f64().unwrap_or(0.0);
    Pose {
        rot: n("rot"),
        head_x: n("headX"),
        head_y: n("headY"),
        face_x: n("faceX"),
        face_y: n("faceY"),
        brow_l: n("browL"),
        brow_r: n("browR"),
        brow_tilt: n("browTilt"),
        jaw: n("jaw"),
        cheek: n("cheek"),
        breath: n("breath"),
        zoom: if v["zoom"].is_null() { 1.0 } else { n("zoom") },
    }
}

/// `warp` (a package's warp), `pose` and `points` (`[x, y]` of the canvas) in; the point each is
/// read from out.
pub(super) fn run_warp(case: &Value) -> Value {
    let warp: Warp = serde_json::from_value(case["warp"].clone())
        .unwrap_or_else(|_| unreachable!("the vector holds a warp"));
    let pose = pose_of(&case["pose"]);
    let points: Vec<Value> = case["points"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|p| {
            numbers(&warp.source_point(
                &pose,
                p[0].as_f64().unwrap_or(0.0),
                p[1].as_f64().unwrap_or(0.0),
            ))
        })
        .collect();
    json!({ "points": points })
}

pub(super) fn warp_cases() -> Vec<Value> {
    let points = json!([
        [10, 10],
        [190, 190],
        [100, 20],
        [84, 62],
        [115, 61],
        [100, 89],
        [100, 106],
        [100, 122],
        [100, 130],
        [100, 150],
        [100, 175],
        [60, 160]
    ]);
    let poses = [
        ("the rest pose moves nothing", json!({})),
        (
            "a head turned and nodding",
            json!({ "rot": 3.0, "headX": 2.0, "headY": -3.0, "faceX": 1.0, "faceY": 1.5 }),
        ),
        (
            "brows raised and tilted",
            json!({ "browL": 4.0, "browR": 2.5, "browTilt": 1.5 }),
        ),
        (
            "a jaw dropped and cheeks lifted",
            json!({ "jaw": 3.0, "cheek": 1.2 }),
        ),
        (
            "a breath and a zoom",
            json!({ "breath": 1.0, "zoom": 1.02 }),
        ),
    ];
    poses
        .iter()
        .map(|(name, pose)| {
            let mut case = json!({ "name": name, "warp": package_json()["warp"], "pose": pose, "points": points });
            case["expected"] = run_warp(&case);
            case
        })
        .collect()
}
