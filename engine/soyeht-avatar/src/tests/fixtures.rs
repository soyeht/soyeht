//! A synthetic avatar and synthetic speech for the tests. The contours are generated from the
//! anchors of the parameter space (aperture from the opening, half width from the width and the
//! rounding), so they are small, regular and carry no picture of the lab.

use serde_json::{Value, json};

use crate::aligner::letters::{Language, Token, phonemize_word, tokens};
use crate::envelope::{Envelope, HOP};
use crate::package::AssetInfo;
use crate::params::Shape;
use crate::phoneme::{Phoneme, STRESS, is_pause, is_semi, vowel};

const CANVAS: u32 = 200;
/// The crop that holds the whole morph, in the canvas: `x, y, width, height`.
const BOX: [f64; 4] = [60.0, 70.0, 80.0, 60.0];
const CENTER: (f64, f64) = (100.0, 100.0);
const RING: usize = 8;

fn ellipse(cx: f64, cy: f64, hw: f64, hh: f64) -> Vec<(f64, f64)> {
    (0..RING)
        .map(|k| {
            let a = 2.0 * std::f64::consts::PI * k as f64 / RING as f64;
            (cx + hw * a.cos(), cy + hh * a.sin())
        })
        .collect()
}

/// The inner and outer lip contours of a drawing, from its anchor: the aperture grows with the
/// opening, the mouth widens with the width and narrows with the rounding, closed lips have none.
type Contour = Vec<(f64, f64)>;

fn contours(shape: Shape) -> (Contour, Contour) {
    let q = shape.anchor();
    let half_width = 18.0 + 14.0 * q.w - 6.0 * q.r;
    let aperture = (1.0 + 13.0 * q.o) * (1.0 - q.c);
    let thickness = 3.0 + 4.0 * q.r + 2.0 * q.c;
    let inner = ellipse(CENTER.0, CENTER.1, half_width, aperture);
    let outer = ellipse(
        CENTER.0,
        CENTER.1,
        half_width + thickness,
        aperture + thickness,
    );
    (inner, outer)
}

fn flat(points: &[(f64, f64)]) -> Vec<f64> {
    points.iter().flat_map(|(x, y)| [*x, *y]).collect()
}

/// The atlas coordinates of a canvas position inside a shape's cell (3 by 3 cells).
fn uv(shape: Shape, points: &[(f64, f64)]) -> Vec<f64> {
    let cell = Shape::ALL.iter().position(|s| *s == shape).unwrap_or(0);
    let (col, row) = ((cell % 3) as f64, (cell / 3) as f64);
    points
        .iter()
        .flat_map(|(x, y)| {
            [
                (col * BOX[2] + x - BOX[0]) / (3.0 * BOX[2]),
                (row * BOX[3] + y - BOX[1]) / (3.0 * BOX[3]),
            ]
        })
        .collect()
}

fn ring_triangles(first: usize, second: usize) -> Vec<usize> {
    (0..RING)
        .flat_map(|k| {
            let k1 = (k + 1) % RING;
            [
                first + k,
                first + k1,
                second + k,
                first + k1,
                second + k1,
                second + k,
            ]
        })
        .collect()
}

fn part(
    id: &str,
    rings: &dyn Fn(Shape) -> Vec<(f64, f64)>,
    triangles: Vec<usize>,
    textured: &dyn Fn(Shape) -> bool,
    alphas: Option<Vec<f64>>,
) -> Value {
    let shapes: Vec<Value> = Shape::ALL
        .iter()
        .map(|s| {
            let points = rings(*s);
            let mut shape = json!({ "id": s.id(), "positions": flat(&points) });
            if textured(*s) {
                shape["uvs"] = json!(uv(*s, &points));
            }
            shape
        })
        .collect();
    let mut value = json!({ "id": id, "triangles": triangles, "shapes": shapes });
    if let Some(alphas) = alphas {
        value["alphas"] = json!(alphas);
    }
    value
}

const PATCH_IDS: [&str; 8] = [
    "half", "left", "right", "up", "down", "upleft", "soft", "blink",
];
const PATCH_SIZE: (u32, u32) = (40, 16);

/// A small valid package document.
pub(super) fn package_json() -> Value {
    // Skin: the outer lip contour and the boundary of the morph, which the drawings share.
    let boundary = ellipse(CENTER.0, CENTER.1, BOX[2] / 2.0, BOX[3] / 2.0);
    let skin = |s: Shape| [contours(s).1, boundary.clone()].concat();
    let lips = |s: Shape| {
        let (inner, outer) = contours(s);
        [outer, inner].concat()
    };
    let inner = |s: Shape| {
        let (inner, _) = contours(s);
        [inner, vec![CENTER]].concat()
    };
    let centre_fan: Vec<usize> = (0..RING).flat_map(|k| [k, (k + 1) % RING, RING]).collect();
    let closed = |s: Shape| !matches!(s, Shape::Rest | Shape::ClosedMbp);
    let mut feather = vec![1.0; RING];
    feather.extend(vec![0.0; RING]);
    let patches = |ids: &[&str], y: u32| -> Vec<Value> {
        ids.iter().enumerate().map(|(i, id)| json!({ "id": id, "file": format!("frames/{id}.png"), "rect": [10 + 45 * (i as u32 % 4), y, PATCH_SIZE.0, PATCH_SIZE.1] })).collect()
    };
    let mut files = vec![
        json!({ "path": "base.png", "width": CANVAS, "height": CANVAS }),
        json!({ "path": "mouth_atlas.png", "width": 240, "height": 180 }),
    ];
    files.extend(PATCH_IDS.iter().map(|id| json!({ "path": format!("frames/{id}.png"), "width": PATCH_SIZE.0, "height": PATCH_SIZE.1 })));
    let mut inner_part = part("inner", &inner, centre_fan, &closed, None);
    inner_part["hideWhenEmpty"] = json!(true);
    json!({
        "format": "soyeht-avatar",
        "version": "0.1",
        "canvas": { "width": CANVAS, "height": CANVAS },
        "files": files,
        "base": "base.png",
        "mouth": {
            "atlas": { "file": "mouth_atlas.png", "columns": 3, "rows": 3 },
            "parts": [
                part("skin", &skin, ring_triangles(0, RING), &|_| true, Some(feather)),
                part("lips", &lips, ring_triangles(0, RING), &|_| true, None),
                inner_part,
            ],
        },
        "eyes": patches(&PATCH_IDS, 30),
        "warp": {
            "pivot": [100, 127],
            "zoomCenter": [100, 109],
            "head": { "center": [100, 75], "radii": [72, 76], "inner": 0.74 },
            "headFade": [117, 143],
            "face": { "center": [100, 89], "radii": [40, 42], "inner": 0.55 },
            "brows": [
                { "center": [84, 62], "radii": [15, 5], "inner": 0.45 },
                { "center": [115, 61], "radii": [15, 5], "inner": 0.45 },
            ],
            "browTiltSpan": 14,
            "jaw": { "center": [100, 106], "radii": [29, 19], "inner": 0.5 },
            "cheek": { "center": [100, 92], "radii": [37, 11], "inner": 0.3 },
            "chest": { "from": 125, "to": 156, "centerX": 100, "spread": 0.006, "rise": 0.7 },
        },
    })
}

/// The assets that go with `package_json`.
pub(super) fn assets() -> Vec<AssetInfo> {
    let mut out = vec![
        AssetInfo {
            path: "base.png".into(),
            bytes: 900,
            size: Some((CANVAS, CANVAS)),
        },
        AssetInfo {
            path: "mouth_atlas.png".into(),
            bytes: 700,
            size: Some((240, 180)),
        },
    ];
    out.extend(PATCH_IDS.iter().map(|id| AssetInfo {
        path: format!("frames/{id}.png"),
        bytes: 120,
        size: Some(PATCH_SIZE),
    }));
    out
}

// ------------------------------------------------------------------------------------ speech

/// Speech whose phoneme timing is known, and the envelope of its voice.
pub(super) struct Speech {
    pub phonemes: Vec<Phoneme>,
    pub envelope: Envelope,
}

fn duration(p: char) -> f64 {
    if p == STRESS {
        0.0
    } else if is_pause(p) {
        if p == ',' { 0.28 } else { 0.4 }
    } else if is_semi(p) {
        0.07
    } else if vowel(p).is_some() {
        0.12
    } else if matches!(p, 'p' | 'b' | 'm' | 'f' | 'v') {
        0.085
    } else {
        0.075
    }
}

/// The phonemes of `text` with the timing of a speaker, `gap` seconds of silence between words and
/// the stressed vowel a little longer. The silences of the pause marks are part of their phonemes.
pub(super) fn speech(text: &str, language: Language, gap: f64) -> Speech {
    let mut phonemes = Vec::new();
    let mut t = 0.0;
    let mut first = true;
    for token in tokens(text) {
        match token {
            Token::Word(word) => {
                if !first && phonemes.last().is_some_and(|p: &Phoneme| !is_pause(p.p)) {
                    t += gap;
                }
                first = false;
                let mut stressed = false;
                for p in phonemize_word(&word, language) {
                    let stretch = if stressed && vowel(p).is_some() {
                        1.35
                    } else {
                        1.0
                    };
                    stressed = p == STRESS;
                    let d = duration(p) * if p == STRESS { 1.0 } else { stretch };
                    phonemes.push(Phoneme {
                        p,
                        t0: t,
                        t1: t + d,
                    });
                    t += d;
                }
            }
            Token::Pause(p) => {
                phonemes.push(Phoneme {
                    p,
                    t0: t,
                    t1: t + duration(p),
                });
                t += duration(p);
            }
        }
    }
    let envelope = envelope_of(&phonemes, t + 0.2);
    Speech { phonemes, envelope }
}

/// The voice level per hop: vowels loud, consonants quieter, silences and pauses quiet.
fn envelope_of(phonemes: &[Phoneme], length: f64) -> Envelope {
    let frames = (length / HOP).ceil() as usize;
    let db = (0..frames)
        .map(|i| {
            let t = (i as f64 + 0.5) * HOP;
            match phonemes.iter().find(|p| p.t0 <= t && t < p.t1) {
                Some(p) if is_pause(p.p) => -80.0,
                Some(p) if vowel(p.p).is_some() => -18.0,
                Some(_) => -32.0,
                None => -80.0,
            }
        })
        .collect();
    Envelope { db, hop: HOP }
}

/// A few sentences of pt-BR with plenty of M, B and P, and some F and V.
pub(super) const PORTUGUESE: &str = "Olá, tudo bem? Eu sou o sábio, mamãe me deu um bom livro. \
    Vamos bater um papo sobre a vida, o mundo e as pessoas que passam pela ponte. Fale baixo, por favor.";

pub(super) const ENGLISH: &str = "Hello, my name is Maria. Please bring the big blue book, maybe tomorrow. Very good, thank you.";

/// Whether a phoneme is a lip closure.
pub(super) fn is_closure(p: &Phoneme) -> bool {
    matches!(p.p, 'p' | 'b' | 'm') && p.t1 - p.t0 >= 0.03
}

impl Shape {
    /// The place of the drawing in the order of the nine weights.
    pub(super) fn index(self) -> usize {
        Shape::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// The twelve numbers of a pose, in the order of the vector files.
pub(super) fn pose_values(p: &crate::acting::Pose) -> [f64; 12] {
    [
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
    ]
}
