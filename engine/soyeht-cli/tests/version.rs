#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `soyeht --version` and `soyeht version` print the product version as JSON, like every other
//! command, and need neither a valid profile nor a running engine.

use serde_json::{Value, json};
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soyeht"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn the_version_flag_prints_the_product_version_and_exits_zero() {
    let output = run(&["--version"]);
    assert!(output.status.success());
    assert_eq!(
        stdout_json(&output),
        json!({"version": env!("CARGO_PKG_VERSION")})
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn the_version_command_prints_the_product_version_and_exits_zero() {
    let output = run(&["version"]);
    assert!(output.status.success());
    assert_eq!(
        stdout_json(&output),
        json!({"version": env!("CARGO_PKG_VERSION")})
    );
}

#[test]
fn the_version_command_needs_no_profile_and_no_engine() {
    let output = run(&["--profile", "no-such-profile", "version"]);
    assert!(output.status.success());
    assert_eq!(
        stdout_json(&output)["version"],
        json!(env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn the_version_command_takes_no_arguments() {
    let output = run(&["version", "extra"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
