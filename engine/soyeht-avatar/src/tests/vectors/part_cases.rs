//! Vector cases about how a part of the mouth is drawn and which pictures it may have: the part
//! laid over a picture once (`composite`), the order the parts are drawn in (`drawOrder`), the area
//! a drawing may cover, and the drawings that have no picture. They go in `avatar-package.json`
//! with the load rules (`package`).

use serde_json::{Value, json};

use super::numbers;
use super::package::{remove, replace};
use crate::mesh::composite;
use crate::package::Package;
use crate::tests::fixtures::{assets, package_json};

/// The area the triangles of the resting drawing cover, the three parts together, and the same
/// scaled about the middle of the canvas.
fn rest_area(package: &Package) -> f64 {
    package
        .mouth
        .parts
        .iter()
        .map(|part| {
            let rest = &part.shapes[0].positions;
            part.triangles
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    let at = |i: u32| (rest[2 * i as usize], rest[2 * i as usize + 1]);
                    let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
                    0.5 * ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs()
                })
                .sum::<f64>()
        })
        .sum()
}

/// The rest drawing blown up until its triangles cover `share` canvas areas (the limit is one).
fn draw_work_case(package: &Package, share: f64, name: &str, expect: &str) -> Value {
    let canvas = f64::from(package.canvas.width) * f64::from(package.canvas.height);
    let factor = (share * canvas / rest_area(package)).sqrt();
    let middle = [
        f64::from(package.canvas.width) / 2.0,
        f64::from(package.canvas.height) / 2.0,
    ];
    let patch: Vec<Value> = package
        .mouth
        .parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let scaled: Vec<f64> = part.shapes[0]
                .positions
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| {
                    [
                        ((middle[0] + (p[0] - middle[0]) * factor) * 1e5).round() / 1e5,
                        ((middle[1] + (p[1] - middle[1]) * factor) * 1e5).round() / 1e5,
                    ]
                })
                .collect();
            replace(
                &format!("/mouth/parts/{i}/shapes/0/positions"),
                json!(scaled),
            )
        })
        .collect();
    json!({ "name": name, "patch": patch, "expect": expect })
}

/// A drawing with no picture of its own: a part that hides when empty (the inner mouth) may have
/// some, whatever area they cover, as the lab's model has (its resting inner mouth covers a
/// sliver); a part that is always drawn may not, since another drawing would take its place in one
/// frame.
pub(super) fn unpictured_cases() -> Vec<Value> {
    let package = Package::load(&package_json().to_string(), &assets())
        .unwrap_or_else(|_| unreachable!("the base is valid"));
    let case = |name: &str, patch: Vec<Value>, expect: &str| json!({ "name": name, "patch": patch, "expect": expect });
    vec![
        case(
            "a drawing of the inner mouth with no picture and the area of an open mouth",
            vec![replace(
                "/mouth/parts/2/shapes/0/positions",
                json!(package.mouth.parts[2].shapes[2].positions),
            )],
            "ok",
        ),
        case(
            "a drawing of a part that is always drawn with no picture",
            vec![remove("/mouth/parts/0/shapes/3/uvs")],
            "value",
        ),
    ]
}

/// A drawing whose triangles cover more than one canvas is refused: what a renderer fills for a
/// frame is bounded.
pub(super) fn draw_work_cases() -> Vec<Value> {
    let package = Package::load(&package_json().to_string(), &assets())
        .unwrap_or_else(|_| unreachable!("the base is valid"));
    vec![
        draw_work_case(&package, 0.99, "a drawing covering 0.99 canvases", "ok"),
        draw_work_case(
            &package,
            1.01,
            "a drawing covering 1.01 canvases",
            "tooLarge",
        ),
    ]
}

pub(super) fn colour_of(v: &Value) -> [f64; 4] {
    let at = |i: usize| v[i].as_f64().unwrap_or(0.0);
    [at(0), at(1), at(2), at(3)]
}

/// `under`, `layers` (`texel`, `alpha`) and `coverage` in; the colour out.
pub(super) fn run_composite(case: &Value) -> Value {
    let layers: Vec<([f64; 4], f64)> = case["layers"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|l| (colour_of(&l["texel"]), l["alpha"].as_f64().unwrap_or(0.0)))
        .collect();
    let colour = composite(
        colour_of(&case["under"]),
        &layers,
        case["coverage"].as_f64().unwrap_or(0.0),
    );
    json!({ "colour": numbers(&colour) })
}

/// A part laid over a picture once: one to three layers, three coverages, three texel alphas, and
/// a layer that is all but absent.
pub(super) fn composite_cases() -> Vec<Value> {
    // Straight colours of the drawings, premultiplied by the alpha of the texel.
    let straight = [
        [200.0, 60.0, 20.0],
        [20.0, 160.0, 220.0],
        [240.0, 240.0, 40.0],
    ];
    let texel = |k: usize, alpha: f64| {
        let c = straight[k];
        json!(numbers(&[
            c[0] * alpha / 255.0,
            c[1] * alpha / 255.0,
            c[2] * alpha / 255.0,
            alpha
        ]))
    };
    // The alphas of the layers of three drawings of weights 0.5, 0.3 and 0.2.
    let alphas = [1.0, 0.375, 0.2];
    let mut cases = Vec::new();
    for count in 1..=3 {
        for coverage in [0.3, 0.5, 1.0] {
            for alpha in [0.0, 128.0, 255.0] {
                let layers: Vec<Value> = (0..count)
                    .map(|k| json!({ "texel": texel(k, alpha), "alpha": alphas[k] }))
                    .collect();
                let mut case = json!({
                    "name": format!("{count} layers, coverage {coverage}, texel alpha {alpha}"),
                    "under": [120, 80, 40, 255], "layers": layers, "coverage": coverage,
                });
                case["expected"] = run_composite(&case);
                cases.push(case);
            }
        }
    }
    let clear = json!({
        "name": "over a picture that is clear",
        "under": [0, 0, 0, 0],
        "layers": [{ "texel": texel(0, 255.0), "alpha": 1.0 }, { "texel": texel(1, 128.0), "alpha": 0.5 }],
        "coverage": 0.5,
    });
    // A drawing that comes in at a weight of 0.0002 changes a feathered pixel by under a 255th.
    let one = json!({
        "name": "a feathered pixel, one layer",
        "under": [120, 80, 40, 255],
        "layers": [{ "texel": texel(0, 255.0), "alpha": 1.0 }],
        "coverage": 0.5,
    });
    let two = json!({
        "name": "the same with a second layer at the weight 0.0002",
        "under": [120, 80, 40, 255],
        "layers": [{ "texel": texel(0, 255.0), "alpha": 1.0 }, { "texel": texel(1, 255.0), "alpha": 0.0002 }],
        "coverage": 0.5,
    });
    for mut case in [clear, one, two] {
        case["expected"] = run_composite(&case);
        cases.push(case);
    }
    cases
}

/// `parts`: the ids of the base package's parts, in the order the package lists them. Out: the
/// ids of the parts in the order they are drawn (a part the list leaves out is not drawn).
pub(super) fn run_draw_order(package: &Package, case: &Value) -> Value {
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

pub(super) fn draw_order_cases(package: &Package) -> Vec<Value> {
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
