//! The cases of `avatar-package.json` that pin the loader's limits and its hostile-input rules:
//! at every limit the package loads, one past it fails.

use serde_json::{Value, json};

use super::package::replace;
use crate::acting_tune::ActingTune;
use crate::package::acting_bounds::BOUNDS;
use crate::package::limits::{
    MAX_DOCUMENT_BYTES, MAX_FILE_BYTES, MAX_FILES, MAX_NAME_BYTES, MAX_PATH_BYTES, MAX_PIXELS,
    MAX_SIDE, MAX_TOTAL_BYTES, MAX_TRIANGLES, MAX_VERTICES,
};
use crate::params::Shape;
use crate::tests::fixtures::package_json;

fn case(name: &str, patch: Vec<Value>, assets: Value, expect: &str) -> Value {
    json!({ "name": name, "patch": patch, "assets": assets, "expect": expect })
}

fn append(path: &str, value: Value, times: usize) -> Value {
    json!({ "op": "append", "path": path, "value": value, "times": times })
}

fn repeat(path: &str, pattern: Value, times: usize) -> Value {
    json!({ "op": "repeat", "path": path, "pattern": pattern, "times": times })
}

/// A file entry of the document and the asset that goes with it.
fn extra_file(path: &str, width: u64, height: u64) -> (Value, Value) {
    (
        json!({ "path": path, "width": width, "height": height }),
        json!({ "path": path, "bytes": 100, "size": [width, height] }),
    )
}

// ------------------------------------------------------------------------------------ paths

pub(super) fn path_cases() -> Vec<Value> {
    // Each path is added as a file the document lists, with its asset: only the path is odd.
    let paths = [
        ("a path that leaves the folder", "a/../b.png", "file"),
        ("an empty name", "a//b.png", "file"),
        ("a name that is only an extension", ".png", "file"),
        ("a hidden name", "dir/.hidden.png", "file"),
        ("a Windows reserved name", "CON.png", "file"),
        ("a Windows reserved name in lower case", "nul.png", "file"),
        (
            "a Windows reserved name with two extensions",
            "dir/Aux.v2.png",
            "file",
        ),
        ("a Windows reserved device number", "com1.png", "file"),
        ("a Windows reserved printer number", "LPT9.png", "file"),
        ("a name with two dots", "name..png", "ok"),
        ("a directory that ends in a dot", "dir./a.png", "file"),
        ("a space", "a b.png", "file"),
        ("a right-to-left override", "a\u{202e}gnp.png", "file"),
        ("a full-width letter", "\u{ff41}.png", "file"),
        ("a control character", "a\u{1}b.png", "file"),
        ("an upper-case extension", "a.PNG", "file"),
        ("a backslash", "a\\b.png", "file"),
        ("a drive", "c:/a.png", "file"),
        ("an underscore, a dash and digits", "dir_1/a-2.png", "ok"),
        (
            "a name of the longest length",
            &format!("{}.png", "n".repeat(MAX_NAME_BYTES - 4)),
            "ok",
        ),
        (
            "a name one over the longest",
            &format!("{}.png", "n".repeat(MAX_NAME_BYTES - 3)),
            "file",
        ),
        (
            "a path of the longest length",
            &long_path(MAX_PATH_BYTES),
            "ok",
        ),
        (
            "a path one over the longest",
            &long_path(MAX_PATH_BYTES + 1),
            "file",
        ),
    ];
    let mut cases: Vec<Value> = paths
        .iter()
        .map(|(name, path, expect)| {
            let (file, asset) = extra_file(path, 4, 4);
            case(
                name,
                vec![append("/files", file, 1)],
                json!({ "add": [asset] }),
                expect,
            )
        })
        .collect();
    // Two names that differ only in case are one file on some systems.
    let (file, asset) = extra_file("BASE.png", 200, 200);
    cases.push(case(
        "two names that differ only in case",
        vec![append("/files", file, 1)],
        json!({ "add": [asset] }),
        "file",
    ));
    cases
}

/// A path of exactly `len` bytes made of names of 8 letters: `aaaaaaaa/aaaaaaaa/.../aa.png`.
fn long_path(len: usize) -> String {
    let tail = "x.png";
    let mut path = String::new();
    while path.len() + 9 + tail.len() < len {
        path.push_str("aaaaaaaa/");
    }
    let fill = len - path.len() - ".png".len();
    path.push_str(&"b".repeat(fill));
    path.push_str(".png");
    path
}

// ------------------------------------------------------------------------------------ warp

pub(super) fn warp_cases() -> Vec<Value> {
    let warp = |name: &str, path: &str, value: Value| {
        case(name, vec![replace(path, value)], json!({}), "warp")
    };
    vec![
        warp(
            "a neck outside the canvas",
            "/warp/pivot",
            json!([100, 500]),
        ),
        warp(
            "a zoom centre outside the canvas",
            "/warp/zoomCenter",
            json!([-1, 100]),
        ),
        warp("a head with no radius", "/warp/head/radii", json!([0, 70])),
        warp(
            "a head wider than the canvas",
            "/warp/head/radii",
            json!([201, 70]),
        ),
        warp(
            "a head whose full part fills the radii",
            "/warp/head/inner",
            json!(1.0),
        ),
        warp(
            "a head whose full part is negative",
            "/warp/head/inner",
            json!(-0.1),
        ),
        warp(
            "a head fade in the wrong order",
            "/warp/headFade",
            json!([140, 120]),
        ),
        warp(
            "a head fade below the picture",
            "/warp/headFade",
            json!([120, 201]),
        ),
        warp(
            "a brow outside the canvas",
            "/warp/brows/1/center",
            json!([300, 60]),
        ),
        case(
            "brows swapped, the left one right of the right one",
            vec![
                replace("/warp/brows/0/center", json!([115, 61])),
                replace("/warp/brows/1/center", json!([84, 62])),
            ],
            json!({}),
            "warp",
        ),
        warp(
            "a brow tilt span of nothing",
            "/warp/browTiltSpan",
            json!(0),
        ),
        warp("a jaw with no radius", "/warp/jaw/radii", json!([30, 0])),
        warp(
            "a chest that spreads too much",
            "/warp/chest/spread",
            json!(0.06),
        ),
        warp(
            "a chest that rises too much",
            "/warp/chest/rise",
            json!(4.01),
        ),
        warp(
            "a chest fade in the wrong order",
            "/warp/chest/from",
            json!(160),
        ),
        case(
            "a chest at the limits",
            vec![
                replace("/warp/chest/spread", json!(0.05)),
                replace("/warp/chest/rise", json!(4.0)),
            ],
            json!({}),
            "ok",
        ),
        case(
            "a base picture that is not the canvas",
            vec![
                replace("/files/0/width", json!(16)),
                replace("/files/0/height", json!(16)),
            ],
            json!({ "size": { "base.png": [16, 16] } }),
            "canvas",
        ),
    ]
}

// ---------------------------------------------------------------------------------- limits

/// The cell of a drawing in the 3 by 3 atlas, as the centre of its texture coordinates (four
/// decimals, inside the cell).
fn cell_centre(shape: Shape) -> [f64; 2] {
    let cell = Shape::ALL.iter().position(|s| *s == shape).unwrap_or(0);
    let centre = |i: usize| [0.1667, 0.5, 0.8333][i];
    [centre(cell % 3), centre(cell / 3)]
}

/// Patches that give part 0 (the skin) `n` vertices in every shape.
fn vertices_patches(n: usize) -> Vec<Value> {
    let mut patches = Vec::new();
    for (i, shape) in Shape::ALL.iter().enumerate() {
        let at = format!("/mouth/parts/0/shapes/{i}");
        patches.push(repeat(&format!("{at}/positions"), json!([100, 100]), n));
        patches.push(repeat(&format!("{at}/uvs"), json!(cell_centre(*shape)), n));
    }
    patches.push(repeat("/mouth/parts/0/alphas", json!([1.0]), n));
    patches
}

/// The size of every file the base document lists, summed.
fn base_pixels() -> u64 {
    package_json()["files"].as_array().map_or(0, |files| {
        files
            .iter()
            .map(|f| f["width"].as_u64().unwrap_or(0) * f["height"].as_u64().unwrap_or(0))
            .sum()
    })
}

pub(super) fn limit_cases() -> Vec<Value> {
    let side = u64::from(MAX_SIDE);
    let canvas = |w: u64, h: u64| {
        vec![
            replace("/canvas/width", json!(w)),
            replace("/canvas/height", json!(h)),
            replace("/files/0/width", json!(w.min(side))),
            replace("/files/0/height", json!(h.min(side))),
        ]
    };
    let mut cases = vec![
        case(
            "a canvas of the largest side",
            canvas(side, side),
            json!({ "size": { "base.png": [side, side] } }),
            "ok",
        ),
        case(
            "a canvas one past the largest side",
            canvas(side + 1, side),
            json!({ "size": { "base.png": [side, side] } }),
            "canvas",
        ),
        case(
            "a picture one past the largest side",
            vec![replace("/files/1/width", json!(side + 1))],
            json!({ "size": { "mouth_atlas.png": [side + 1, 180] } }),
            "assetSize",
        ),
    ];
    // Files: the document lists 10 (base, atlas, eight eye frames).
    let listed = package_json()["files"].as_array().map_or(0, Vec::len);
    for (name, count, expect) in [
        ("the most files", MAX_FILES, "ok"),
        ("one file over the most", MAX_FILES + 1, "tooLarge"),
    ] {
        let (file, _) = extra_file("extra/f{n}.png", 4, 4);
        let add = json!({ "path": "extra/f{n}.png", "bytes": 100, "size": [4, 4], "times": count - listed });
        cases.push(case(
            name,
            vec![append("/files", file, count - listed)],
            json!({ "add": [add] }),
            expect,
        ));
    }
    // Bytes: one file, then all the files together.
    let base_bytes = |n: u64| json!({ "bytes": { "base.png": n } });
    cases.push(case(
        "a file of the most bytes",
        vec![],
        base_bytes(MAX_FILE_BYTES),
        "ok",
    ));
    cases.push(case(
        "a file one byte over the most",
        vec![],
        base_bytes(MAX_FILE_BYTES + 1),
        "tooLarge",
    ));
    let others: u64 = 8 * 120;
    for (name, atlas, expect) in [
        (
            "all the files of the most bytes",
            MAX_TOTAL_BYTES - MAX_FILE_BYTES - others,
            "ok",
        ),
        (
            "all the files one byte over the most",
            MAX_TOTAL_BYTES - MAX_FILE_BYTES - others + 1,
            "tooLarge",
        ),
    ] {
        cases.push(case(
            name,
            vec![],
            json!({ "bytes": { "base.png": MAX_FILE_BYTES, "mouth_atlas.png": atlas } }),
            expect,
        ));
    }
    // Decoded pixels: files that fill the limit exactly, then one pixel more.
    let rest = MAX_PIXELS - base_pixels();
    let (full, remainder) = (rest / (side * side), rest % (side * side));
    let rows = remainder / side;
    let last = remainder % side;
    for (name, extra, expect) in [
        ("pictures that decode to the most pixels", 0, "ok"),
        ("pictures that decode to one pixel over", 1, "tooLarge"),
    ] {
        let mut sizes: Vec<(u64, u64)> = (0..full).map(|_| (side, side)).collect();
        sizes.push((side, rows));
        sizes.push((last + extra, 1));
        let (files, assets): (Vec<Value>, Vec<Value>) = sizes
            .iter()
            .enumerate()
            .map(|(i, (w, h))| extra_file(&format!("px{i}.png"), *w, *h))
            .unzip();
        let patches = files.into_iter().map(|f| append("/files", f, 1)).collect();
        cases.push(case(name, patches, json!({ "add": assets }), expect));
    }
    cases.extend(mesh_cases());
    cases
}

/// The mesh (vertices of a part and triangles) and the document itself.
fn mesh_cases() -> Vec<Value> {
    let pad = |bytes: usize| json!({ "name": format!("a document of {bytes} bytes"), "patch": [], "documentBytes": bytes });
    let mut cases = Vec::new();
    for (name, n, expect) in [
        ("a part with the most vertices", MAX_VERTICES, "ok"),
        (
            "a part with one vertex over the most",
            MAX_VERTICES + 1,
            "tooLarge",
        ),
    ] {
        cases.push(case(name, vertices_patches(n), json!({}), expect));
    }
    for (name, n, expect) in [
        ("a part with the most triangles", MAX_TRIANGLES, "ok"),
        (
            "a part with one triangle over the most",
            MAX_TRIANGLES + 1,
            "tooLarge",
        ),
    ] {
        cases.push(case(
            name,
            vec![repeat("/mouth/parts/1/triangles", json!([0, 0, 0]), n)],
            json!({}),
            expect,
        ));
    }
    // The document itself, before it is parsed.
    for (bytes, expect) in [
        (MAX_DOCUMENT_BYTES, "ok"),
        (MAX_DOCUMENT_BYTES + 1, "tooLarge"),
    ] {
        let mut c = pad(bytes);
        c["expect"] = json!(expect);
        cases.push(c);
    }
    cases
}

// ---------------------------------------------------------------------------------- acting

/// A number for a slot of the default tune: a whole number where the tune holds one.
fn number_like(slot: &Value, value: f64) -> Value {
    if slot.is_u64() {
        json!(value.round().max(0.0) as u64)
    } else {
        json!(value)
    }
}

/// The default tune as a document, with the value at `pointer` replaced.
fn tune_with_value(pointer: &str, value: Value) -> Value {
    let mut tune = serde_json::to_value(ActingTune::default()).unwrap_or(Value::Null);
    if let Some(slot) = tune.pointer_mut(pointer) {
        *slot = value;
    }
    tune
}

/// The default tune as a document, with the number at `pointer` set to `value`.
fn tune_with(pointer: &str, value: f64) -> Value {
    let default = serde_json::to_value(ActingTune::default()).unwrap_or(Value::Null);
    let slot = default.pointer(pointer).cloned().unwrap_or(Value::Null);
    tune_with_value(pointer, number_like(&slot, value))
}

/// Every constant of the acting layer a package can set is pinned: all at their least load, all at
/// their most load, and each one a little under its least and over its most fails.
pub(super) fn acting_cases() -> Vec<Value> {
    let all = |pick: fn(&(&str, f64, f64, &str)) -> f64| {
        let mut tune = serde_json::to_value(ActingTune::default()).unwrap_or(Value::Null);
        for bound in BOUNDS {
            if let Some(slot) = tune.pointer_mut(bound.0) {
                *slot = number_like(slot, pick(bound));
            }
        }
        tune
    };
    let mut cases = vec![
        case(
            "every acting constant at its least",
            vec![replace("/acting", all(|b| b.1))],
            json!({}),
            "ok",
        ),
        case(
            "every acting constant at its most",
            vec![replace("/acting", all(|b| b.2))],
            json!({}),
            "ok",
        ),
        case(
            "a constant that is null",
            vec![replace("/acting", json!({ "head": { "nodStart": null } }))],
            json!({}),
            "malformed",
        ),
        case(
            "the acting constants of a tune with fewer fields",
            vec![replace("/acting", json!({ "head": { "nodStart": 20.0 } }))],
            json!({}),
            "ok",
        ),
        case(
            "a gap range that runs backwards",
            vec![replace(
                "/acting",
                tune_with_value("/blink/gap", json!([5.0, 2.0])),
            )],
            json!({}),
            "acting",
        ),
        case(
            "a gap range of one value",
            vec![replace(
                "/acting",
                tune_with_value("/blink/gap", json!([2.0, 2.0])),
            )],
            json!({}),
            "ok",
        ),
    ];
    for (pointer, least, most, _) in BOUNDS {
        let step = (least.abs().max(most.abs()) * 0.01).max(0.001);
        for (side, value) in [
            ("under its least", least - step),
            ("over its most", most + step),
        ] {
            cases.push(case(
                &format!("{pointer} {side}"),
                vec![replace("/acting", tune_with(pointer, value))],
                json!({}),
                "acting",
            ));
        }
    }
    cases
}
