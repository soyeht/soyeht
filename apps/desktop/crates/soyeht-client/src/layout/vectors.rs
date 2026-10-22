//! The cases of `contracts/vectors/pane-layout.json`, which the Mac client passes as well.

use super::*;
use crate::testing::{cases, pane_json, vector};
use serde_json::{Value, json};

fn tree(value: &Value) -> Node {
    serde_json::from_value(value.clone()).unwrap()
}

fn name(case: &Value) -> &str {
    case["name"].as_str().unwrap()
}

fn live(case: &Value) -> Vec<Pane> {
    case["live"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let mut json = pane_json(
                entry["paneId"].as_str().unwrap(),
                "w",
                entry["mode"].as_str().unwrap(),
            );
            if let Some(kind) = entry.get("writingKind") {
                json["writingKind"] = kind.clone();
            }
            serde_json::from_value(json).unwrap()
        })
        .collect()
}

fn gap() -> f32 {
    vector("pane-layout")["gap"].as_f64().unwrap() as f32
}

fn floor() -> f32 {
    vector("pane-layout")["floor"]["width"].as_f64().unwrap() as f32
}

#[test]
fn normalizing_merges_and_collapses_as_the_mac_does() {
    for case in cases("pane-layout", "normalize") {
        assert_eq!(
            normalize(tree(&case["tree"])),
            tree(&case["expected"]),
            "{}",
            name(&case)
        );
    }
}

#[test]
fn splitting_places_the_new_pane_as_the_mac_does() {
    for case in cases("pane-layout", "split") {
        let new = &case["newPane"];
        let fresh: Pane = serde_json::from_value(pane_json(
            new["paneId"].as_str().unwrap(),
            "w",
            new["mode"].as_str().unwrap(),
        ))
        .unwrap();
        let axis: Axis = serde_json::from_value(case["axis"].clone()).unwrap();
        let placed = split_pane(
            &tree(&case["tree"]),
            case["anchor"].as_str().unwrap(),
            &fresh,
            axis,
        );
        let expected = (!case["expected"].is_null()).then(|| tree(&case["expected"]));
        assert_eq!(placed, expected, "{}", name(&case));
    }
}

#[test]
fn a_stored_arrangement_is_read_as_the_mac_reads_it() {
    for case in cases("pane-layout", "stored") {
        let panes = live(&case);
        let refs: Vec<&Pane> = panes.iter().collect();
        let expected = (!case["expected"].is_null()).then(|| tree(&case["expected"]));
        assert_eq!(
            stored_tree(&case["layout"], &refs).map(|(tree, _)| tree),
            expected,
            "{}",
            name(&case)
        );
    }
}

#[test]
fn the_width_a_tree_needs_is_the_mac_minimum() {
    for case in cases("pane-layout", "minimumWidth") {
        assert_eq!(
            minimum_width(&tree(&case["tree"]), floor(), gap()),
            case["minimumWidth"].as_f64().unwrap() as f32,
            "{}",
            name(&case)
        );
    }
}

#[test]
fn panes_sit_where_the_mac_places_them_at_the_minimum_width() {
    for case in cases("pane-layout", "paneSpans") {
        let node = tree(&case["tree"]);
        for (id, span) in case["spans"].as_object().unwrap() {
            let expected = (
                span[0].as_f64().unwrap() as f32,
                span[1].as_f64().unwrap() as f32,
            );
            assert_eq!(
                pane_span(&node, id, floor(), gap()),
                Some(expected),
                "{}: {id}",
                name(&case)
            );
        }
        assert_eq!(pane_span(&node, "gone", floor(), gap()), None);
    }
}

fn sizing_of(case: &Value) -> Sizing {
    let all = vector("pane-layout");
    let pair = |value: &Value| (value[0].as_f64().unwrap(), value[1].as_f64().unwrap());
    let floor = &all["floor"];
    Sizing {
        floor: (
            floor["width"].as_f64().unwrap(),
            floor["height"].as_f64().unwrap(),
        ),
        kinds: case["floors"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(kind, size)| (serde_json::from_value(json!(kind)).unwrap(), pair(size)))
            .collect(),
        horizontal_gap: all["gap"].as_f64().unwrap(),
        vertical_gap: all["gap"].as_f64().unwrap(),
        companion_strip: all["companionStrip"].as_f64().unwrap(),
    }
}

#[test]
fn panes_the_tree_does_not_list_are_arranged_as_the_mac_arranges_them() {
    for case in cases("pane-layout", "arrange") {
        let panes = live(&case);
        let refs: Vec<&Pane> = panes.iter().collect();
        assert_eq!(
            resolve(&case["layout"], &refs, &sizing_of(&case)),
            Some(tree(&case["expected"])),
            "{}",
            name(&case)
        );
    }
}

fn path_of(case: &Value) -> Vec<usize> {
    case["splitPath"]
        .as_array()
        .unwrap()
        .iter()
        .map(|index| index.as_u64().unwrap() as usize)
        .collect()
}

fn viewport_of(case: &Value) -> (f64, f64) {
    (
        case["viewport"]["width"].as_f64().unwrap(),
        case["viewport"]["height"].as_f64().unwrap(),
    )
}

/// Whether two trees are the same but for the last digits of a weight: `serde_json` reads a
/// decimal to within a unit in the last place unless told to round-trip it, and a weight beside
/// a floor is the difference of numbers that differ by a billionth.
fn same_tree(first: &Node, second: &Node) -> bool {
    match (first, second) {
        (Node::V1(first), Node::V1(second)) => {
            first.axis == second.axis
                && first.companion == second.companion
                && first.children.len() == second.children.len()
                && first.children.iter().zip(&second.children).all(|(a, b)| {
                    (a.weight - b.weight).abs() <= 1e-15 * a.weight.abs().max(b.weight.abs())
                        && same_tree(&a.node, &b.node)
                })
        }
        _ => first == second,
    }
}

fn assert_same(found: Result<Node, Refused>, case: &Value) {
    let expected = (!case["expected"].is_null()).then(|| tree(&case["expected"]));
    let same = match (&found, &expected) {
        (Ok(found), Some(expected)) => same_tree(found, expected),
        (Err(_refused), None) => true,
        _ => false,
    };
    assert!(same, "{}: {found:?} is not {expected:?}", name(case));
}

#[test]
fn dragging_a_divider_resizes_as_the_mac_does() {
    for case in cases("pane-layout", "resize") {
        let resized = resize(
            &tree(&case["tree"]),
            &path_of(&case),
            case["dividerIndex"].as_u64().unwrap() as usize,
            case["delta"].as_f64().unwrap(),
            viewport_of(&case),
            &sizing_of(&case),
        );
        assert_same(resized, &case);
    }
}

#[test]
fn equalizing_a_split_shares_it_as_the_mac_does() {
    for case in cases("pane-layout", "equalize") {
        let equalized = equalize(
            &tree(&case["tree"]),
            &path_of(&case),
            viewport_of(&case),
            &sizing_of(&case),
        );
        assert_same(equalized, &case);
    }
}
