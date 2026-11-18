//! Opens one avatar from a package folder (`src/avatar/demo.rs`). Usage:
//! `cargo run --release --features dev-tools --example avatar_stage -- <folder> [theme]`, where the
//! folder holds `avatar.json` and the pictures it lists and the theme is the end of a theme name
//! without blanks (`milk`, `deepharbor`, ...).

use soyeht_desktop::theme::ThemeName;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(folder) = args.next() else {
        eprintln!("usage: avatar_stage <package folder> [theme]");
        std::process::exit(2);
    };
    let wanted = args.next().unwrap_or_else(|| "milk".into());
    match ThemeName::named(&wanted) {
        Some(theme) => soyeht_desktop::avatar::run_stage(PathBuf::from(folder), theme),
        None => {
            eprintln!("no theme named {wanted}");
            std::process::exit(2);
        }
    }
}
