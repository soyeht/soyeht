//! The vectors of the package loader: `avatar-package.json`.

use serde_json::{Value, json};

use super::{read, render, updating, vectors_dir};
use crate::package::{AssetInfo, Package};
use crate::tests::fixtures::{assets, package_json};

// ---------------------------------------------------------------------------- package cases

fn apply(doc: &mut Value, patch: &Value) {
    let path = patch["path"].as_str().unwrap_or("");
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

fn replace(path: &str, value: Value) -> Value {
    json!({ "op": "replace", "path": path, "value": value })
}

fn remove(path: &str) -> Value {
    json!({ "op": "remove", "path": path })
}

fn version_cases() -> Vec<Value> {
    let case = |name: &str, patch: Vec<Value>, expect: &str| json!({ "name": name, "patch": patch, "expect": expect });
    vec![
        case("a valid package", vec![], "ok"),
        case(
            "a later minor version and a field it adds",
            vec![
                replace("/version", "1.9".into()),
                replace("/extra", json!({ "x": 1 })),
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
            "another major version",
            vec![replace("/version", "2.0".into())],
            "unsupportedVersion",
        ),
        case(
            "a version with no minor",
            vec![replace("/version", "1".into())],
            "unsupportedVersion",
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
            "a resting mouth missing",
            vec![remove("/restMouths/1")],
            "frames",
        ),
        case(
            "a warp lattice with no columns",
            vec![replace("/warp/cols", 0.into())],
            "warp",
        ),
        case(
            "a warp lattice outside the canvas",
            vec![replace("/warp/rect", json!([150, 150, 100, 100]))],
            "warp",
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
    [version_cases(), file_cases(), mesh_cases(), frame_cases()].concat()
}

fn assets_of(base: &[AssetInfo], change: &Value) -> Vec<AssetInfo> {
    let removed: Vec<&str> = change["remove"]
        .as_array()
        .map_or(vec![], |a| a.iter().filter_map(Value::as_str).collect());
    base.iter()
        .filter(|a| !removed.contains(&a.path.as_str()))
        .map(|a| {
            let mut a = a.clone();
            if let Some(size) = change["size"].get(&a.path) {
                a.size = size
                    .as_array()
                    .and_then(|s| Some((s.first()?.as_u64()? as u32, s.get(1)?.as_u64()? as u32)));
            }
            if let Some(bytes) = change["bytes"].get(&a.path).and_then(Value::as_u64) {
                a.bytes = bytes;
            }
            a
        })
        .collect()
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
    match Package::load(&doc.to_string(), &assets) {
        Ok(_) => "ok".to_string(),
        Err(e) => e.kind.as_str().to_string(),
    }
}

fn package_vectors() -> Value {
    let base_assets = assets();
    json!({
        "description": "The load rules of an avatar package (soyeht-avatar). `base` is a valid document and `assets` the files the caller has (size in bytes, and the PNG's width and height or null). Each case applies its `patch` (replace or remove at a JSON pointer) to `base`, may change the assets (`assets`: `remove` paths, `size` and `bytes` by path), and expects `ok` or the kind of the first rule it breaks.",
        "base": package_json(),
        "assets": asset_values(&base_assets),
        "cases": package_cases(),
    })
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
