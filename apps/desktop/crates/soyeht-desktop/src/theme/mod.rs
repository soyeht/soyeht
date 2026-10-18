//! The Soyeht palette tokens: the one place the desktop client gets a color from (ADR 0065).
//!
//! A view asks the active [`Palette`] for a role. It never writes a color literal and never
//! derives a new color. The palettes and every numeric token (radii, spacing, type scale,
//! shadows, layout metrics, fonts) are generated from `contracts/design-tokens.json`, the same
//! source the Mac client is generated from (`generated.rs`).

mod generated;

pub use generated::{
    ThemeName, composer, desktop, editor, elevation, fonts, glyph, layout, pane_interior,
    pane_layout, pane_shell, radius, space, type_scale, usage_ring,
};

use soyeht_client::screen::CellColor;

/// Hex colors as `0xRRGGBB`, one field per role of `SoyehtThemePalette`.
#[derive(Debug, PartialEq, Eq)]
pub struct Palette {
    pub screen: u32,
    pub canvas: u32,
    pub face: u32,
    pub hover: u32,
    pub well: u32,
    pub ink: u32,
    pub secondary: u32,
    pub muted: u32,
    pub accent: u32,
    pub on_accent: u32,
    pub shadow_dark: u32,
    pub shadow_light: u32,
    pub well_shadow: u32,
    pub well_rim: u32,
    pub selection: u32,
    pub presentation_letterbox: u32,
    pub well_lip: Option<u32>,
    pub agent_plates: &'static [u32],
    pub ansi: [u32; 16],
    pub syntax_type: u32,
    pub diff_added: u32,
}

impl Palette {
    pub fn danger(&self) -> u32 {
        self.ansi[1]
    }

    pub fn success(&self) -> u32 {
        self.ansi[2]
    }

    pub fn warning(&self) -> u32 {
        self.ansi[3]
    }

    /// The color of a terminal cell. The first sixteen indexed colors are the palette's ANSI
    /// slots; the rest follow the xterm 6x6x6 cube and grayscale ramp every program expects.
    pub fn terminal(&self, color: CellColor) -> u32 {
        match color {
            CellColor::DefaultFg => self.ink,
            CellColor::DefaultBg => self.face,
            CellColor::Rgb(r, g, b) => u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b),
            CellColor::Indexed(index) if index < 16 => self.ansi[usize::from(index)],
            CellColor::Indexed(index @ 16..=231) => {
                let cube = u32::from(index - 16);
                let level = |step: u32| if step == 0 { 0 } else { 55 + 40 * step };
                level(cube / 36) << 16 | level(cube / 6 % 6) << 8 | level(cube % 6)
            }
            CellColor::Indexed(index) => {
                let gray = 8 + 10 * u32::from(index - 232);
                gray << 16 | gray << 8 | gray
            }
        }
    }
}

impl ThemeName {
    /// The theme whose name, without example, ends with `wanted` (`milk`, `deepharbor`, ...):
    /// how the examples that compare themes are told which one to draw.
    pub fn named(wanted: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|theme| {
            let name = theme
                .display_name()
                .replace([' ', '\u{b7}'], "")
                .to_lowercase();
            name.ends_with(&wanted.to_lowercase())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seeded_default_is_neo_milk() {
        assert_eq!(ThemeName::DEFAULT, ThemeName::NeoMilk);
        assert_eq!(ThemeName::DEFAULT.palette().canvas, 0xE0E5EC);
        assert_eq!(ThemeName::DEFAULT.display_name(), "Neo \u{b7} Milk");
    }

    #[test]
    fn a_theme_is_found_by_the_end_of_its_name() {
        assert_eq!(ThemeName::named("Milk"), Some(ThemeName::NeoMilk));
        assert_eq!(
            ThemeName::named("deepharbor"),
            Some(ThemeName::NeoDeepHarbor)
        );
        assert_eq!(ThemeName::named("nothing"), None);
    }

    #[test]
    fn semantic_colors_are_ansi_slots() {
        let palette = ThemeName::NeoMilk.palette();
        assert_eq!(palette.danger(), palette.ansi[1]);
        assert_eq!(palette.success(), palette.ansi[2]);
        assert_eq!(palette.warning(), palette.ansi[3]);
    }

    #[test]
    fn terminal_colors_use_the_palette_then_the_xterm_ramps() {
        let palette = ThemeName::NeoMilk.palette();
        assert_eq!(palette.terminal(CellColor::DefaultFg), palette.ink);
        assert_eq!(palette.terminal(CellColor::DefaultBg), palette.face);
        assert_eq!(palette.terminal(CellColor::Indexed(1)), palette.ansi[1]);
        assert_eq!(palette.terminal(CellColor::Indexed(16)), 0x000000);
        assert_eq!(palette.terminal(CellColor::Indexed(196)), 0xFF0000);
        assert_eq!(palette.terminal(CellColor::Indexed(231)), 0xFFFFFF);
        assert_eq!(palette.terminal(CellColor::Indexed(232)), 0x080808);
        assert_eq!(palette.terminal(CellColor::Indexed(255)), 0xEEEEEE);
        assert_eq!(palette.terminal(CellColor::Rgb(1, 2, 3)), 0x010203);
    }

    #[test]
    fn every_theme_has_its_own_palette_and_the_generated_tokens_are_reachable() {
        let canvases: std::collections::HashSet<u32> = ThemeName::ALL
            .iter()
            .map(|theme| theme.palette().canvas)
            .collect();
        assert_eq!(canvases.len(), ThemeName::ALL.len());
        assert_eq!(radius::PILL, 999.0);
        assert_eq!(elevation::BALLOON.dark_offset, 6.0);
        assert_eq!(fonts::UI, "Inter");
    }
}
