//! The vector files the Swift and the GPUI renderers pass too (ADR 0166, section 6):
//! `contracts/vectors/avatar-mouth.json` (weights, tracks, the mouth's motion, the aligner and the
//! acting layer) and `contracts/vectors/avatar-package.json` (the load rules).
//!
//! The inputs are written in the file and each case is run again from them, so the file is what
//! is tested. After a deliberate change of the runtime, rewrite the files with
//! `SOYEHT_UPDATE_VECTORS=1 cargo test -p soyeht-avatar vectors` and look at the diff: a vector
//! that changes is a mouth that changes, and goes through the checker limits and a contact sheet.

mod mouth;
mod package;
mod package_limits;
mod runtime;

use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::{Value, json};

/// The tolerance another implementation must meet, and the rounding of the files.
pub(super) const TOLERANCE: f64 = 1e-3;
pub(super) const DECIMALS: f64 = 1e5;
pub(super) const FPS: f64 = 60.0;

pub(super) fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/vectors")
}

pub(super) fn round(x: f64) -> Value {
    json!((x * DECIMALS).round() / DECIMALS)
}

pub(super) fn numbers(xs: &[f64]) -> Value {
    Value::Array(xs.iter().map(|x| round(*x)).collect())
}

// ------------------------------------------------------------------------------- the files

/// Indent a value, but keep an array or object on one line when it holds only scalars or arrays
/// of scalars, so a sample or a phoneme is one line.
fn write(v: &Value, depth: usize, out: &mut String) {
    let scalar = |v: &Value| !v.is_array() && !v.is_object();
    let leaf = |v: &Value| scalar(v) || v.as_array().is_some_and(|a| a.iter().all(scalar));
    let compact = match v {
        Value::Array(a) => a.iter().all(leaf),
        Value::Object(o) => o.values().all(leaf),
        _ => true,
    };
    if compact || depth > 6 {
        out.push_str(&serde_json::to_string(v).unwrap_or_default());
        return;
    }
    let pad = "  ".repeat(depth + 1);
    let (open, close) = if v.is_array() { ('[', ']') } else { ('{', '}') };
    out.push(open);
    out.push('\n');
    let items: Vec<(Option<&String>, &Value)> = match v {
        Value::Array(a) => a.iter().map(|x| (None, x)).collect(),
        Value::Object(o) => o.iter().map(|(k, x)| (Some(k), x)).collect(),
        _ => vec![],
    };
    for (i, (key, item)) in items.iter().enumerate() {
        out.push_str(&pad);
        if let Some(key) = key {
            let _ = write!(out, "{}: ", serde_json::to_string(key).unwrap_or_default());
        }
        write(item, depth + 1, out);
        out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
    }
    out.push_str(&"  ".repeat(depth));
    out.push(close);
}

fn render(v: &Value) -> String {
    let mut out = String::new();
    write(v, 0, &mut out);
    out.push('\n');
    out
}

/// Numbers within `TOLERANCE`, everything else equal.
fn close(a: &Value, b: &Value, path: &str) -> Result<(), String> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (
                x.as_f64().unwrap_or(f64::NAN),
                y.as_f64().unwrap_or(f64::NAN),
            );
            if (x - y).abs() <= 2.0 / DECIMALS {
                Ok(())
            } else {
                Err(format!("{path}: {x} against {y}"))
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .enumerate()
            .try_for_each(|(i, (p, q))| close(p, q, &format!("{path}/{i}"))),
        (Value::Object(x), Value::Object(y)) if x.len() == y.len() => {
            x.iter().try_for_each(|(k, p)| {
                y.get(k).map_or(Err(format!("{path}/{k}: missing")), |q| {
                    close(p, q, &format!("{path}/{k}"))
                })
            })
        }
        _ if a == b => Ok(()),
        _ => Err(format!("{path}: {a} against {b}")),
    }
}

fn read(name: &str) -> Value {
    let text = std::fs::read_to_string(vectors_dir().join(name))
        .expect("the vector file exists; rewrite it with SOYEHT_UPDATE_VECTORS=1");
    serde_json::from_str(&text).expect("the vector file is JSON")
}

fn updating() -> bool {
    std::env::var_os("SOYEHT_UPDATE_VECTORS").is_some()
}

fn check_group(file: &Value, group: &str, run: fn(&Value) -> Value) {
    let cases = file[group]
        .as_array()
        .unwrap_or_else(|| panic!("{group} is a list"));
    assert!(!cases.is_empty(), "{group} has cases");
    for case in cases {
        let name = case["name"].as_str().unwrap_or("?");
        if let Err(why) = close(&run(case), &case["expected"], "") {
            panic!("{group} / {name}: {why}");
        }
    }
}
