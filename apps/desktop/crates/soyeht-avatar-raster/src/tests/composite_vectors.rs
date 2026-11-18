//! The `composite` cases of `contracts/vectors/avatar-package.json`, which every renderer passes:
//! a part laid over a picture once, each layer over the picture the layers before it made. The
//! rasteriser works in 256ths on bytes, so it agrees with the file's floating point to a couple of
//! units of 255.

use crate::pixmap::{mix, over, pack, unpack};
use std::path::PathBuf;

const UNITS: i32 = 2;

fn colour(v: &serde_json::Value) -> [f64; 4] {
    let at = |i: usize| v[i].as_f64().unwrap_or(f64::NAN);
    [at(0), at(1), at(2), at(3)]
}

fn bytes(c: [f64; 4]) -> [u8; 4] {
    c.map(|v| v.round().clamp(0.0, 255.0) as u8)
}

#[test]
fn the_rasteriser_composites_a_part_as_the_vectors_say() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../contracts/vectors/avatar-package.json");
    let file: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let cases = file["composite"].as_array().unwrap();
    assert!(cases.len() >= 27);
    for case in cases {
        let under = pack(bytes(colour(&case["under"])));
        // The layers are mixed by their weights from a clear picture, and the mix is laid over
        // the picture below once, at the coverage.
        let mut picture = 0;
        for layer in case["layers"].as_array().unwrap() {
            let texel = pack(bytes(colour(&layer["texel"])));
            let covers = (layer["alpha"].as_f64().unwrap() * 256.0).round() as u32;
            picture = mix(picture, texel, covers);
        }
        let covers = (case["coverage"].as_f64().unwrap() * 256.0).round() as u32;
        let got = unpack(over(under, picture, covers));
        let expected = bytes(colour(&case["expected"]["colour"]));
        for (g, e) in got.iter().zip(expected) {
            assert!(
                (i32::from(*g) - i32::from(e)).abs() <= UNITS,
                "{}: {got:?} against {expected:?}",
                case["name"]
            );
        }
    }
}
