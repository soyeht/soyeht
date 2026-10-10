//! The two approved typefaces: Inter for the interface and JetBrains Mono for terminals and
//! code. The font files are the Mac app's own resources, embedded from there so that no
//! second copy exists.

use gpui::App;
use std::borrow::Cow;

pub const UI_FAMILY: &str = "Inter";
pub const MONO_FAMILY: &str = "JetBrains Mono";

/// Interface text size in points.
pub const UI_SIZE: f32 = 13.0;
/// Terminal text size in points, as on the Mac client.
pub const TERMINAL_SIZE: f32 = 13.0;

const FILES: [&[u8]; 4] = [
    include_bytes!("../../../../mac/Resources/Fonts/Inter.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/JetBrainsMono-Medium.ttf"),
];

/// Makes the bundled faces available to the text system.
pub fn register(cx: &App) {
    let faces = FILES.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(error) = cx.text_system().add_fonts(faces) {
        eprintln!("soyeht: bundled fonts could not be loaded: {error}");
    }
}
