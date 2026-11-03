//! What the tests of this crate that read the sources of the workspace share.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Every `.rs` file below `directory`. A folder named `tests` and a file named `tests.rs` are test
/// code; they are left out when `production_only` is set.
pub fn rust_files(directory: &Path, found: &mut Vec<PathBuf>, production_only: bool) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_owned();
        if path.is_dir() {
            if !(production_only && name == "tests") {
                rust_files(&path, found, production_only);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !(production_only && name == "tests.rs")
        {
            found.push(path);
        }
    }
}
