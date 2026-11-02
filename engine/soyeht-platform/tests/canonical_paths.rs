//! Paths are canonicalized in one place. `std::fs::canonicalize` and `Path::canonicalize` give
//! Windows' extended `\\?\C:\...` form, which `cmd.exe` cannot start in, `git` reports
//! differently and a client cannot compare with the path it sent. `soyeht_platform::fs`
//! strips that prefix, so every other crate calls it by its full name.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::rust_files;
use std::{fs, path::Path};

/// The ways of calling the standard canonicalization found on `line`. The method of a path
/// takes no argument; a `.canonicalize(argument)` is a port method of the caller's own, and the
/// platform function is allowed by its full name.
fn violations(line: &str) -> Vec<&'static str> {
    let line = line.replace("soyeht_platform::fs::canonicalize(", "");
    let mut found = Vec::new();
    if line.contains(".canonicalize()") {
        found.push("Path::canonicalize()");
    }
    if line.contains("fs::canonicalize(") {
        found.push("std::fs::canonicalize");
    }
    found
}

#[test]
fn no_crate_but_the_platform_one_uses_the_standard_canonicalize() {
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut offences = Vec::new();
    for entry in fs::read_dir(engine).unwrap() {
        let krate = entry.unwrap().path();
        if !krate.join("Cargo.toml").is_file() || krate.ends_with("soyeht-platform") {
            continue;
        }
        let mut files = Vec::new();
        for folder in ["src", "tests", "benches", "examples"] {
            rust_files(&krate.join(folder), &mut files, false);
        }
        for file in files {
            for (number, line) in fs::read_to_string(&file).unwrap().lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for found in violations(line) {
                    offences.push(format!(
                        "{}:{}: {found}; use soyeht_platform::fs::canonicalize",
                        file.strip_prefix(engine).unwrap().display(),
                        number + 1
                    ));
                }
            }
        }
    }
    assert!(offences.is_empty(), "{}", offences.join("\n"));
}

#[test]
fn the_rule_sees_each_spelling_and_allows_the_platform_function() {
    assert_eq!(
        violations("let a = path.canonicalize()?;"),
        ["Path::canonicalize()"]
    );
    assert_eq!(
        violations("fs::canonicalize(&p)"),
        ["std::fs::canonicalize"]
    );
    assert_eq!(
        violations("std::fs::canonicalize(&p)"),
        ["std::fs::canonicalize"]
    );
    assert!(violations("soyeht_platform::fs::canonicalize(&p)").is_empty());
    assert!(violations("self.files.canonicalize(&p)").is_empty());
    assert!(violations("fn canonicalize(&self, path: &Path)").is_empty());
}
