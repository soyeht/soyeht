//! Opens the neumorphic gallery (`src/gallery.rs`) in one theme, for comparing the surfaces of
//! the desktop client with the Mac's. Usage: `cargo run --example neo_gallery -- milk`, where
//! the argument is the end of a theme name without example (`milk`, `deepharbor`, ...).

use soyeht_desktop::theme::ThemeName;

fn main() {
    let wanted = std::env::args().nth(1).unwrap_or_else(|| "milk".into());
    match ThemeName::named(&wanted) {
        Some(theme) => soyeht_desktop::run_gallery(theme),
        None => {
            eprintln!("no theme named {wanted}");
            std::process::exit(2);
        }
    }
}
