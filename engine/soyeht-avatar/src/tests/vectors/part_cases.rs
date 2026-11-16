//! Vector cases about how a part of the mouth is drawn and which pictures it may have: the part
//! laid over a picture once (`composite`), the order the parts are drawn in (`drawOrder`), the area
//! a drawing may cover, and the drawings that have no picture. They go in `avatar-package.json`
//! with the load rules (`package`).

use serde_json::{Value, json};

use super::numbers;
use super::package::{remove, replace};
use crate::mesh::composite;
use crate::package::{Package, blend_work};
use crate::tests::fixtures::{assets, package_json};

/// The patch that scales the resting drawing of every part by `factor` about the middle of the
/// canvas (positions to 5 decimals, as the file holds them), and the package it makes.
fn scaled_rest(package: &Package, factor: f64) -> (Vec<Value>, Package) {
    let middle = [
        f64::from(package.canvas.width) / 2.0,
        f64::from(package.canvas.height) / 2.0,
    ];
    let mut made = package.clone();
    let mut patch = Vec::new();
    for (i, part) in made.mouth.parts.iter_mut().enumerate() {
        for point in part.shapes[0].positions.as_chunks_mut::<2>().0 {
            for k in 0..2 {
                point[k] = ((middle[k] + (point[k] - middle[k]) * factor) * 1e5).round() / 1e5;
            }
        }
        patch.push(replace(
            &format!("/mouth/parts/{i}/shapes/0/positions"),
            json!(part.shapes[0].positions),
        ));
    }
    (patch, made)
}

/// The resting drawing blown up until the work of the mouth (`blend_work`) is `share` canvases.
fn draw_work_case(package: &Package, share: f64, name: &str, expect: &str) -> Value {
    // The work grows with the factor: find it by halving.
    let (mut low, mut high) = (1.0, 64.0);
    for _ in 0..60 {
        let middle = (low + high) / 2.0;
        if blend_work(&scaled_rest(package, middle).1) < share {
            low = middle;
        } else {
            high = middle;
        }
    }
    let (patch, _) = scaled_rest(package, low);
    json!({ "name": name, "patch": patch, "expect": expect })
}

/// Two drawings of the inner mouth with no area at all (every corner on one line through the
/// centre of the fan, each drawing on its own line) whose corners blend, half and half, into a
/// triangle: the first corner of the fan is `reach` and `2 reach` from the centre along the
/// first line in one drawing, and `2 reach` and `reach` along the second line in the other, the
/// other corners at the centre. The seven other drawings are the base's, around the centre.
fn blended_triangle_case(package: &Package, reach: f64, name: &str, expect: &str) -> Value {
    let rest = &package.mouth.parts[2].shapes[0].positions;
    let centre = [rest[rest.len() - 2], rest[rest.len() - 1]];
    let drawing = |first: [f64; 2], second: [f64; 2]| {
        let mut positions: Vec<f64> = rest.chunks(2).flat_map(|_| centre).collect();
        for (slot, offset) in [(0, first), (2, second)] {
            positions[slot] = centre[0] + offset[0];
            positions[slot + 1] = centre[1] + offset[1];
        }
        json!(positions)
    };
    json!({
        "name": name,
        "patch": [
            replace("/mouth/parts/2/shapes/0/positions", drawing([reach, 0.0], [2.0 * reach, 0.0])),
            replace("/mouth/parts/2/shapes/1/positions", drawing([0.0, 2.0 * reach], [0.0, reach])),
        ],
        "expect": expect,
    })
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

/// The work of a mouth, `blend_work`, is at most one canvas: what a renderer fills for a frame is
/// bounded, however the drawings blend.
pub(super) fn draw_work_cases() -> Vec<Value> {
    let package = Package::load(&package_json().to_string(), &assets())
        .unwrap_or_else(|_| unreachable!("the base is valid"));
    vec![
        draw_work_case(&package, 0.99, "a mouth that works 0.99 canvases", "ok"),
        draw_work_case(
            &package,
            1.01,
            "a mouth that works 1.01 canvases",
            "tooLarge",
        ),
        blended_triangle_case(
            &package,
            40.0,
            "two drawings with no area that blend into triangles of well under a canvas",
            "ok",
        ),
        blended_triangle_case(
            &package,
            60.0,
            "two drawings with no area that blend into triangles of over a canvas",
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
/// a layer that is all but absent, and mixes of clear and opaque texels.
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
    // The drawings are mixed by their weights, whatever the opacity of their texels: a clear texel
    // of a heavy drawing clears the mix in proportion to its weight.
    let mixed = json!({
        "name": "an opaque and a clear texel at equal weights, over a picture",
        "under": [120, 80, 40, 255],
        "layers": [{ "texel": texel(0, 255.0), "alpha": 1.0 }, { "texel": [0, 0, 0, 0], "alpha": 0.5 }],
        "coverage": 1.0,
    });
    // A drawing that comes in under the layer threshold and over it, in front of a drawing of a
    // clear texel that has all the weight: the picture is the same to under a 255th.
    let before = json!({
        "name": "a clear texel with all the weight, the drawing before it under the threshold",
        "under": [120, 80, 40, 255],
        "layers": [{ "texel": [0, 0, 0, 0], "alpha": 1.0 }],
        "coverage": 1.0,
    });
    let after = json!({
        "name": "the same with the drawing before it over the threshold",
        "under": [120, 80, 40, 255],
        "layers": [{ "texel": texel(0, 255.0), "alpha": 1.0 }, { "texel": [0, 0, 0, 0], "alpha": 0.99989 }],
        "coverage": 1.0,
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
    for mut case in [clear, mixed, before, after, one, two] {
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
