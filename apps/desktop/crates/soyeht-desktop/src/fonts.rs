//! The approved typefaces: Nunito for the window chrome, Inter for the interface inside panes
//! and JetBrains Mono for terminals and code (family names come from the design tokens). The
//! font files are the Mac app's own resources, embedded from there so that no second copy
//! exists. The one exception is Inter's weights: GPUI on Linux draws a variable font at its
//! default weight, so the medium, semibold, bold and extra bold cuts are instanced from the Mac's
//! variable `Inter.ttf` by `scripts/instance_inter_fonts.py` and embedded beside it.

use gpui::App;
use std::borrow::Cow;

/// Terminal text size in points, as on the Mac client.
pub const TERMINAL_SIZE: f32 = 13.0;

const FILES: [&[u8]; 12] = [
    include_bytes!("../../../../mac/Resources/Fonts/Inter.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/Inter-ExtraBold.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/JetBrainsMono-Medium.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/JetBrainsMono-SemiBold.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/Nunito-Regular.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/Nunito-SemiBold.ttf"),
    include_bytes!("../../../../mac/Resources/Fonts/Nunito-Bold.ttf"),
];

/// Makes the bundled faces available to the text system.
pub fn register(cx: &App) {
    let faces = FILES.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(error) = cx.text_system().add_fonts(faces) {
        eprintln!("soyeht: bundled fonts could not be loaded: {error}");
    }
}
