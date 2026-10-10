//! The Soyeht palette tokens: the one place the desktop client gets a color from (ADR 0065).
//!
//! A view asks the active [`Palette`] for a role. It never writes a color literal and never
//! derives a new color; the catalogue below is data mirrored from the Swift client, and the
//! parity test fails when the two drift apart.

mod palettes;

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

/// The reviewed themes. Neo Milk is the seeded default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeName {
    NeoMilk,
    NeoPaleMist,
    NeoMistyBlue,
    NeoSunlitChartreuse,
    NeoSunriseGold,
    NeoDeepVine,
    NeoDeepForest,
    NeoMidnightTeal,
    NeoDeepHarbor,
}

impl ThemeName {
    pub const ALL: [Self; 9] = [
        Self::NeoMilk,
        Self::NeoPaleMist,
        Self::NeoMistyBlue,
        Self::NeoSunlitChartreuse,
        Self::NeoSunriseGold,
        Self::NeoDeepVine,
        Self::NeoDeepForest,
        Self::NeoMidnightTeal,
        Self::NeoDeepHarbor,
    ];

    pub const DEFAULT: Self = Self::NeoMilk;

    pub fn palette(self) -> &'static Palette {
        match self {
            Self::NeoMilk => &palettes::MILK,
            Self::NeoPaleMist => &palettes::PALE_MIST,
            Self::NeoMistyBlue => &palettes::MISTY_BLUE,
            Self::NeoSunlitChartreuse => &palettes::SUNLIT_CHARTREUSE,
            Self::NeoSunriseGold => &palettes::SUNRISE_GOLD,
            Self::NeoDeepVine => &palettes::DEEP_VINE,
            Self::NeoDeepForest => &palettes::DEEP_FOREST,
            Self::NeoMidnightTeal => &palettes::MIDNIGHT_TEAL,
            Self::NeoDeepHarbor => &palettes::DEEP_HARBOR,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::NeoMilk => "Neo \u{b7} Milk",
            Self::NeoPaleMist => "Neo \u{b7} Pale Mist",
            Self::NeoMistyBlue => "Neo \u{b7} Misty Blue",
            Self::NeoSunlitChartreuse => "Neo \u{b7} Sunlit Chartreuse",
            Self::NeoSunriseGold => "Neo \u{b7} Sunrise Gold",
            Self::NeoDeepVine => "Neo \u{b7} Deep Vine",
            Self::NeoDeepForest => "Neo \u{b7} Deep Forest",
            Self::NeoMidnightTeal => "Neo \u{b7} Midnight Teal",
            Self::NeoDeepHarbor => "Neo \u{b7} Deep Harbor",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Swift catalogue, the source this module mirrors.
    fn swift_source() -> String {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../shared/Sources/SoyehtClient/ThemePalette.swift"
        );
        std::fs::read_to_string(path).unwrap()
    }

    fn swift_name(theme: ThemeName) -> &'static str {
        match theme {
            ThemeName::NeoMilk => "milk",
            ThemeName::NeoPaleMist => "paleMist",
            ThemeName::NeoMistyBlue => "mistyBlue",
            ThemeName::NeoSunlitChartreuse => "sunlitChartreuse",
            ThemeName::NeoSunriseGold => "sunriseGold",
            ThemeName::NeoDeepVine => "deepVine",
            ThemeName::NeoDeepForest => "deepForest",
            ThemeName::NeoMidnightTeal => "midnightTeal",
            ThemeName::NeoDeepHarbor => "deepHarbor",
        }
    }

    fn block<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source
            .find(&format!("fileprivate static let {name} = make("))
            .unwrap();
        let rest = &source[start..];
        &rest[..rest.find("\n\n").unwrap()]
    }

    fn hex(text: &str) -> u32 {
        u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap()
    }

    fn field(block: &str, label: &str) -> Option<u32> {
        let key = format!("{label}: \"#");
        let at = block.find(&key)? + key.len();
        Some(hex(&block[at..at + 6]))
    }

    fn list(block: &str, label: &str) -> Vec<u32> {
        let key = format!("{label}: [");
        let at = block.find(&key).unwrap() + key.len();
        let body = &block[at..at + block[at..].find(']').unwrap()];
        body.split('"')
            .filter(|piece| piece.starts_with('#'))
            .map(hex)
            .collect()
    }

    #[test]
    fn every_theme_matches_the_swift_catalogue() {
        let source = swift_source();
        for theme in ThemeName::ALL {
            let swift = block(&source, swift_name(theme));
            let rust = theme.palette();
            let expect = |label: &str, value: u32| {
                let found = field(swift, label);
                assert_eq!(found, Some(value), "{theme:?} {label}");
            };
            expect("screen", rust.screen);
            expect("canvas", rust.canvas);
            expect("face", rust.face);
            expect("hover", rust.hover);
            expect("well", rust.well);
            expect("ink", rust.ink);
            expect("secondary", rust.secondary);
            expect("muted", rust.muted);
            expect("accent", rust.accent);
            expect("onAccent", rust.on_accent);
            expect("shadowDark", rust.shadow_dark);
            expect("shadowLight", rust.shadow_light);
            expect("wellShadow", rust.well_shadow);
            expect("wellRim", rust.well_rim);
            expect("selection", rust.selection);
            expect("presentationLetterbox", rust.presentation_letterbox);
            assert_eq!(field(swift, "wellLip"), rust.well_lip, "{theme:?} wellLip");
            assert_eq!(list(swift, "agents"), rust.agent_plates, "{theme:?} agents");
            let ansi = list(swift, "ansi");
            assert_eq!(ansi, rust.ansi, "{theme:?} ansi");
            // The Swift `make` falls back to these two ANSI slots.
            assert_eq!(
                field(swift, "syntaxType").unwrap_or(ansi[13]),
                rust.syntax_type
            );
            assert_eq!(
                field(swift, "diffAdded").unwrap_or(ansi[10]),
                rust.diff_added
            );
        }
    }

    #[test]
    fn the_seeded_default_is_neo_milk() {
        assert_eq!(ThemeName::DEFAULT, ThemeName::NeoMilk);
        assert_eq!(ThemeName::DEFAULT.palette().canvas, 0xE0E5EC);
        assert_eq!(ThemeName::DEFAULT.display_name(), "Neo \u{b7} Milk");
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
}
