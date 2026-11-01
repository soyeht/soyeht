//! TCP listeners are made in one place. On Windows a program of any user can take the port of a
//! listener that did not ask for exclusive use, so `soyeht_platform::net::listen` is the way to
//! listen, and no production code of the engine or the desktop client binds a `TcpListener` itself.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    fs,
    path::{Path, PathBuf},
};

/// The one file that may bind: the seam itself.
const SEAM: &str = "soyeht-platform/src/net.rs";

fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            // A folder of tests is test code.
            if path.file_name().is_some_and(|name| name != "tests") {
                rust_files(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path.file_name().is_some_and(|name| name != "tests.rs")
        {
            found.push(path);
        }
    }
}

/// The lines of `text` that bind a listener, up to its first test module: tests may bind what they
/// like, and this repository puts them after the code they test.
fn binds(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim_start();
        if line.starts_with("#[cfg(test)]") || line.starts_with("#[cfg(all(test") {
            break;
        }
        if !line.starts_with("//") && line.contains("TcpListener::bind(") {
            found.push(number + 1);
        }
    }
    found
}

#[test]
fn no_production_code_binds_a_tcp_listener_but_the_platform_seam() {
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let desktop = engine.parent().unwrap().join("apps/desktop/crates");
    let mut crates: Vec<PathBuf> = fs::read_dir(engine)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    crates.extend(
        fs::read_dir(&desktop)
            .into_iter()
            .flatten()
            .map(|entry| entry.unwrap().path()),
    );
    let mut offences = Vec::new();
    for krate in crates
        .iter()
        .filter(|krate| krate.join("Cargo.toml").is_file())
    {
        let mut files = Vec::new();
        rust_files(&krate.join("src"), &mut files);
        for file in files {
            let relative = file
                .strip_prefix(engine)
                .unwrap_or(&file)
                .display()
                .to_string();
            if relative.replace('\\', "/") == SEAM {
                continue;
            }
            for number in binds(&fs::read_to_string(&file).unwrap()) {
                offences.push(format!(
                    "{relative}:{number}: use soyeht_platform::net::listen"
                ));
            }
        }
    }
    assert!(offences.is_empty(), "{}", offences.join("\n"));
}

#[test]
fn the_rule_sees_a_bind_in_production_code_and_leaves_tests_alone() {
    assert_eq!(binds("let l = TcpListener::bind(a)?;"), [1]);
    assert_eq!(
        binds("let l = tokio::net::TcpListener::bind(a).await?;"),
        [1]
    );
    assert!(binds("// TcpListener::bind(a)").is_empty());
    assert!(
        binds("fn a() {}\n#[cfg(test)]\nmod t { fn b() { TcpListener::bind(a); } }").is_empty()
    );
    assert!(binds("let l = net::listen(a)?;").is_empty());
}
