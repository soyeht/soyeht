//! The vectors of the package loader: `avatar-package.json`.

use serde_json::{Value, json};

use super::{numbers, read, render, round, updating, vectors_dir};
use crate::package::{AssetInfo, Package};
use crate::tests::fixtures::{assets, package_json};

// ---------------------------------------------------------------------------- package cases

/// Replace `{n}` in every string of a value with `n`.
fn numbered(value: &Value, n: usize) -> Value {
    match value {
        Value::String(s) => Value::String(s.replace("{n}", &n.to_string())),
        Value::Array(a) => Value::Array(a.iter().map(|v| numbered(v, n)).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), numbered(v, n))).collect())
        }
        other => other.clone(),
    }
}

/// Apply one patch. `replace` sets (and adds a missing member of an object), `remove` deletes,
/// `append` pushes `times` copies of `value` onto an array (`{n}` in a string is the copy's
/// number), and `repeat` sets an array to `pattern` repeated `times`.
fn apply(doc: &mut Value, patch: &Value) {
    let path = patch["path"].as_str().unwrap_or("");
    let times = patch["times"].as_u64().unwrap_or(1) as usize;
    match patch["op"].as_str() {
        Some("append") => {
            if let Some(Value::Array(a)) = doc.pointer_mut(path) {
                a.extend((0..times).map(|n| numbered(&patch["value"], n)));
            }
            return;
        }
        Some("repeat") => {
            let pattern = patch["pattern"].as_array().cloned().unwrap_or_default();
            if let Some(target) = doc.pointer_mut(path) {
                *target = Value::Array(
                    pattern
                        .iter()
                        .cycle()
                        .take(pattern.len() * times)
                        .cloned()
                        .collect(),
                );
            }
            return;
        }
        _ => {}
    }
    let (parent, key) = path.rsplit_once('/').unwrap_or(("", path));
    let Some(target) = doc.pointer_mut(parent) else {
        return;
    };
    match (patch["op"].as_str(), target) {
        (Some("remove"), Value::Array(a)) => {
            if let Ok(i) = key.parse::<usize>() {
                a.remove(i);
            }
        }
        (Some("remove"), Value::Object(o)) => {
            o.remove(key);
        }
        (_, Value::Array(a)) => {
            if let Ok(i) = key.parse::<usize>() {
                a[i] = patch["value"].clone();
            }
        }
        (_, Value::Object(o)) => {
            o.insert(key.to_string(), patch["value"].clone());
        }
        _ => {}
    }
}

pub(super) fn replace(path: &str, value: Value) -> Value {
    json!({ "op": "replace", "path": path, "value": value })
}

pub(super) fn remove(path: &str) -> Value {
    json!({ "op": "remove", "path": path })
}

/// Versions that are not `0.1`, each spelled a way a loose reader might accept.
fn version_string_cases() -> Vec<Value> {
    [
        "0.2", "1.0", "0", "0.", "0.x", "00.1", "0.01", "+0.1", "-0.1", " 0.1", "0.1 ", "0.1.0", "1",
        "1.", "1.x", "+1.0", "01.0", "1.99999999999999999999",
    ]
    .iter()
    .map(|v| {
        json!({ "name": format!("the version {v:?}"), "patch": [replace("/version", json!(v))], "expect": "unsupportedVersion" })
    })
    .collect()
}

fn version_cases() -> Vec<Value> {
    let case = |name: &str, patch: Vec<Value>, expect: &str| json!({ "name": name, "patch": patch, "expect": expect });
    vec![
        case("a valid package", vec![], "ok"),
        case(
            "a field this version does not know is ignored",
            vec![
                replace("/extra", json!({ "x": 1 })),
                replace("/restMouths", json!([])),
            ],
            "ok",
        ),
        case(
            "the acting constants are optional",
            vec![remove("/acting")],
            "ok",
        ),
        case(
            "another format",
            vec![replace("/format", "other".into())],
            "wrongFormat",
        ),
        case(
            "a canvas that is not a size",
            vec![replace("/canvas", "big".into())],
            "malformed",
        ),
        case(
            "an empty canvas",
            vec![replace("/canvas/width", 0.into())],
            "canvas",
        ),
        case(
            "a canvas that is too large",
            vec![replace("/canvas/width", 5000.into())],
            "canvas",
        ),
    ]
}

fn file_cases() -> Vec<Value> {
    let case = |name: &str, patch: Vec<Value>, expect: &str| json!({ "name": name, "patch": patch, "expect": expect });
    let with_assets = |name: &str, change: Value, expect: &str| json!({ "name": name, "patch": [], "assets": change, "expect": expect });
    vec![
        case(
            "a path that leaves the folder",
            vec![
                replace("/files/0/path", "../base.png".into()),
                replace("/base", "../base.png".into()),
            ],
            "file",
        ),
        case(
            "an absolute path",
            vec![
                replace("/files/0/path", "/base.png".into()),
                replace("/base", "/base.png".into()),
            ],
            "file",
        ),
        case(
            "a file that is not a png",
            vec![
                replace("/files/0/path", "base.jpg".into()),
                replace("/base", "base.jpg".into()),
            ],
            "file",
        ),
        case(
            "a file listed twice",
            vec![replace("/files/1/path", "base.png".into())],
            "file",
        ),
        case(
            "a file named and not listed",
            vec![replace("/base", "missing.png".into())],
            "file",
        ),
        with_assets(
            "a listed file the caller does not have",
            json!({ "remove": ["base.png"] }),
            "missingAsset",
        ),
        with_assets(
            "a picture of another size than declared",
            json!({ "size": { "base.png": [100, 100] } }),
            "assetSize",
        ),
        with_assets(
            "a file that is not a png",
            json!({ "size": { "base.png": null } }),
            "assetSize",
        ),
        with_assets(
            "a file that is too large",
            json!({ "bytes": { "base.png": 5_000_000 } }),
            "tooLarge",
        ),
    ]
}

fn mesh_cases() -> Vec<Value> {
    let case = |name: &str, patch: Vec<Value>, expect: &str| json!({ "name": name, "patch": patch, "expect": expect });
    let shape = "/mouth/parts/0/shapes";
    vec![
        case(
            "four shapes only",
            vec![remove(&format!("{shape}/8")), remove(&format!("{shape}/7"))],
            "shapes",
        ),
        case(
            "a shape twice",
            vec![replace(&format!("{shape}/3/id"), "A".into())],
            "shapes",
        ),
        case(
            "an unknown shape",
            vec![replace(&format!("{shape}/3/id"), "Z".into())],
            "shapes",
        ),
        case(
            "a shape with another vertex count",
            vec![replace(&format!("{shape}/3/positions"), json!([1.0, 2.0]))],
            "topology",
        ),
        case("a part missing", vec![remove("/mouth/parts/2")], "topology"),
        case(
            "two skins",
            vec![replace("/mouth/parts/1/id", "skin".into())],
            "topology",
        ),
        case(
            "a triangle that names no vertex",
            vec![replace("/mouth/parts/1/triangles/4", 9999.into())],
            "index",
        ),
        case(
            "a triangle list that is not whole triangles",
            vec![replace("/mouth/parts/1/triangles", json!([0, 1]))],
            "topology",
        ),
        case(
            "an alpha above one",
            vec![replace("/mouth/parts/0/alphas/2", 2.0.into())],
            "value",
        ),
        case(
            "a position far outside the canvas",
            vec![replace(&format!("{shape}/2/positions/0"), 1.0e6.into())],
            "value",
        ),
        case(
            "a texture coordinate outside its cell",
            vec![replace(&format!("{shape}/4/uvs/0"), 0.99.into())],
            "value",
        ),
        case(
            "a texture power under one",
            vec![replace("/mouth/parts/0/texturePower", 0.5.into())],
            "value",
        ),
        case(
            "an atlas with too few cells",
            vec![
                replace("/mouth/atlas/columns", 2.into()),
                replace("/mouth/atlas/rows", 2.into()),
            ],
            "atlas",
        ),
    ]
}

fn frame_cases() -> Vec<Value> {
    let case = |name: &str, patch: Vec<Value>, expect: &str| json!({ "name": name, "patch": patch, "expect": expect });
    vec![
        case("an eye frame missing", vec![remove("/eyes/3")], "frames"),
        case(
            "an eye frame outside the canvas",
            vec![replace("/eyes/0/rect/0", 190.into())],
            "frames",
        ),
        case(
            "a blink gap in the wrong order",
            vec![replace(
                "/acting",
                json!({ "blink": { "gap": [5.0, 2.0] } }),
            )],
            "acting",
        ),
        case(
            "a chance above one",
            vec![replace("/acting", json!({ "stress": { "chance": 1.5 } }))],
            "acting",
        ),
    ]
}

/// The cases, grouped by what they break: `expect` is `ok` or an `ErrorKind`.
fn package_cases() -> Vec<Value> {
    [
        version_cases(),
        version_string_cases(),
        file_cases(),
        mesh_cases(),
        frame_cases(),
        super::package_limits::path_cases(),
        super::package_limits::warp_cases(),
        super::package_limits::limit_cases(),
        super::package_limits::acting_cases(),
    ]
    .concat()
}

fn assets_of(base: &[AssetInfo], change: &Value) -> Vec<AssetInfo> {
    let removed: Vec<&str> = change["remove"]
        .as_array()
        .map_or(vec![], |a| a.iter().filter_map(Value::as_str).collect());
    let size_of = |v: &Value| {
        v.as_array()
            .and_then(|s| Some((s.first()?.as_u64()? as u32, s.get(1)?.as_u64()? as u32)))
    };
    let mut out: Vec<AssetInfo> = base
        .iter()
        .filter(|a| !removed.contains(&a.path.as_str()))
        .map(|a| {
            let mut a = a.clone();
            if let Some(size) = change["size"].get(&a.path) {
                a.size = size_of(size);
            }
            if let Some(bytes) = change["bytes"].get(&a.path).and_then(Value::as_u64) {
                a.bytes = bytes;
            }
            a
        })
        .collect();
    // `add`: files the caller has beyond the base's, `times` copies each, `{n}` the copy's number.
    for entry in change["add"].as_array().map_or(&[][..], Vec::as_slice) {
        for n in 0..entry["times"].as_u64().unwrap_or(1) as usize {
            let entry = numbered(entry, n);
            out.push(AssetInfo {
                path: entry["path"].as_str().unwrap_or("").to_string(),
                bytes: entry["bytes"].as_u64().unwrap_or(0),
                size: size_of(&entry["size"]),
            });
        }
    }
    out
}

fn asset_values(list: &[AssetInfo]) -> Value {
    Value::Array(list.iter().map(|a| json!({ "path": a.path, "bytes": a.bytes, "size": a.size.map(|(w, h)| json!([w, h])) })).collect())
}

fn asset_list(v: &Value) -> Vec<AssetInfo> {
    v.as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|a| AssetInfo {
            path: a["path"].as_str().unwrap_or("").to_string(),
            bytes: a["bytes"].as_u64().unwrap_or(0),
            size: a["size"]
                .as_array()
                .and_then(|s| Some((s.first()?.as_u64()? as u32, s.get(1)?.as_u64()? as u32))),
        })
        .collect()
}

fn package_outcome(base: &Value, assets: &[AssetInfo], case: &Value) -> String {
    let mut doc = base.clone();
    for patch in case["patch"].as_array().map_or(&[][..], Vec::as_slice) {
        apply(&mut doc, patch);
    }
    let assets = assets_of(assets, &case["assets"]);
    let mut text = doc.to_string();
    // `documentBytes`: blanks follow the JSON until the document is exactly that many bytes.
    if let Some(bytes) = case["documentBytes"].as_u64() {
        text.extend(std::iter::repeat_n(
            ' ',
            (bytes as usize).saturating_sub(text.len()),
        ));
    }
    match Package::load(&text, &assets) {
        Ok(_) => "ok".to_string(),
        Err(e) => e.kind.as_str().to_string(),
    }
}

/// The blend and the picture layers of the base package's mouth at some morph weights (the nine,
/// in the order of the shapes). Out: for each part, its vertex positions and its layers.
fn run_mesh(package: &Package, case: &Value) -> Value {
    let list = case["weights"].as_array().map_or(&[][..], Vec::as_slice);
    let mut weights = [0.0; 9];
    for (slot, v) in weights.iter_mut().zip(list) {
        *slot = v.as_f64().unwrap_or(0.0);
    }
    let parts: Vec<Value> = package
        .mouth
        .parts
        .iter()
        .map(|part| {
            let layers: Vec<Value> = part
                .layers(&weights)
                .iter()
                .map(|l| json!({ "shape": l.shape.id(), "alpha": round(l.alpha) }))
                .collect();
            json!({ "id": part.id, "positions": numbers(&part.blend(&weights)), "layers": layers })
        })
        .collect();
    json!({ "parts": parts })
}

fn mesh_blend_cases(package: &Package) -> Vec<Value> {
    // Weights in the order rest, closed_MBP, A, E, I, O, U, FV, LDT.
    let sets: [(&str, [f64; 9]); 8] = [
        (
            "the rest drawing alone",
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        (
            "the A drawing alone",
            [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        (
            "half way between closed and A",
            [0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        (
            "three drawings",
            [0.2, 0.1, 0.3, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        (
            "A and E, the nearest ones dominate the skin",
            [0.0, 0.0, 0.7, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        (
            "a weight under the threshold is blended but not drawn",
            [0.0, 0.0, 0.99999, 0.00001, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        (
            "all nine, evenly",
            [
                0.111_111_111_111,
                0.111_111_111_111,
                0.111_111_111_111,
                0.111_111_111_111,
                0.111_111_111_111,
                0.111_111_111_111,
                0.111_111_111_111,
                0.111_111_111_111,
                0.111_111_111_112,
            ],
        ),
        (
            "the closed drawing, whose inner mouth has no picture",
            [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
    ];
    sets.iter()
        .map(|(name, w)| {
            let mut case = json!({ "name": name, "weights": numbers(w) });
            case["expected"] = run_mesh(package, &case);
            case
        })
        .collect()
}

/// `parts`: the ids of the base package's parts, in the order the package lists them. Out: the
/// ids of the parts in the order they are drawn (a part the list leaves out is not drawn).
fn run_draw_order(package: &Package, case: &Value) -> Value {
    let mut model = package.mouth.clone();
    let listed: Vec<String> = case["parts"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let id = |p: &crate::package::MeshPart| json!(p.id).as_str().unwrap_or("").to_string();
    model.parts = listed
        .iter()
        .filter_map(|name| package.mouth.parts.iter().find(|p| id(p) == *name).cloned())
        .collect();
    let order: Vec<String> = model.in_drawing_order().into_iter().map(id).collect();
    json!({ "order": order })
}

fn draw_order_cases(package: &Package) -> Vec<Value> {
    let lists: [&[&str]; 8] = [
        &["skin", "lips", "inner"],
        &["skin", "inner", "lips"],
        &["lips", "skin", "inner"],
        &["lips", "inner", "skin"],
        &["inner", "skin", "lips"],
        &["inner", "lips", "skin"],
        &["lips", "skin"],
        &["inner"],
    ];
    lists
        .iter()
        .map(|parts| {
            let mut case =
                json!({ "name": format!("listed {}", parts.join(", ")), "parts": parts });
            case["expected"] = run_draw_order(package, &case);
            case
        })
        .collect()
}

fn package_vectors() -> Value {
    let base_assets = assets();
    json!({
        "description": "The mesh of the base package: `drawOrder` cases give the ids of the mouth's parts in the order the package lists them and expect the order they are drawn in, each over the one before: skin, lips, inner (a part the list lacks is skipped). `mesh` cases give nine morph weights (rest, closed_MBP, A, E, I, O, U, FV, LDT) and expect, for each part, the blended vertex positions (the weighted sum of the shapes' positions) and the picture layers (the textured shapes in that order whose weight is over 0.0001, each with an alpha of its weight raised to the part's texture power over the sum so far; when none takes part, the first textured shape alone with alpha 1, unless the part hides when empty). The load rules of an avatar package (soyeht-avatar). `base` is a valid document and `assets` the files the caller has (size in bytes, and the PNG's width and height or null). Each case applies its `patch` to `base` and expects `ok` or the kind of the first rule it breaks. Patch operations, each with a JSON `path`: `replace` (sets the value, and adds a missing member of an object), `remove`, `append` (pushes `times` copies of `value` onto an array; `{n}` in a string is the copy's number from 0) and `repeat` (sets an array to `pattern` repeated `times`). A case may change the assets: `remove` paths, `size` and `bytes` by path, and `add` (entries with `path`, `bytes`, `size`, `times`, as `append`). `documentBytes` makes the document exactly that many bytes by following the JSON with blanks. The cases at and just past every limit of the loader pin it: a reader that loads the one past, or refuses the one at, has other limits.",
        "base": package_json(),
        "assets": asset_values(&base_assets),
        "cases": package_cases(),
        "mesh": mesh_blend_cases(&base_package(&base_assets)),
        "drawOrder": draw_order_cases(&base_package(&base_assets)),
    })
}

fn base_package(base_assets: &[AssetInfo]) -> Package {
    Package::load(&package_json().to_string(), base_assets)
        .unwrap_or_else(|_| unreachable!("the base is valid"))
}

#[test]
fn vectors_package_file_matches_the_loader() {
    if updating() {
        std::fs::write(
            vectors_dir().join("avatar-package.json"),
            render(&package_vectors()),
        )
        .expect("write");
    }
    let file = read("avatar-package.json");
    let assets = asset_list(&file["assets"]);
    let cases = file["cases"].as_array().expect("cases");
    let package = Package::load(&file["base"].to_string(), &assets).expect("the base loads");
    for case in file["mesh"].as_array().expect("mesh") {
        if let Err(why) = super::close(&run_mesh(&package, case), &case["expected"], "") {
            panic!("mesh / {}: {why}", case["name"]);
        }
    }
    for case in file["drawOrder"].as_array().expect("drawOrder") {
        if let Err(why) = super::close(&run_draw_order(&package, case), &case["expected"], "") {
            panic!("drawOrder / {}: {why}", case["name"]);
        }
    }
    let mut kinds = std::collections::BTreeSet::new();
    for case in cases {
        let want = case["expect"].as_str().unwrap_or("?");
        assert_eq!(
            package_outcome(&file["base"], &assets, case),
            want,
            "{}",
            case["name"]
        );
        kinds.insert(want.to_string());
    }
    // Every failure kind the loader has is exercised by a case.
    for kind in [
        "malformed",
        "wrongFormat",
        "unsupportedVersion",
        "canvas",
        "file",
        "missingAsset",
        "assetSize",
        "tooLarge",
        "shapes",
        "topology",
        "index",
        "value",
        "frames",
        "warp",
        "acting",
        "atlas",
        "ok",
    ] {
        assert!(kinds.contains(kind), "no case for {kind}");
    }
}
