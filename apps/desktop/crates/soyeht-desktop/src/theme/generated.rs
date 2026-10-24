// Generated from contracts/design-tokens.json. Do not edit.
use super::Palette;

/// The reviewed themes, in the order of `palettes` in the token file. The first one is the
/// seeded default.
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
    pub const ALL: &[Self] = &[
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
    pub const DEFAULT: Self = Self::ALL[0];

    pub fn palette(self) -> &'static Palette {
        match self {
            Self::NeoMilk => &MILK,
            Self::NeoPaleMist => &PALE_MIST,
            Self::NeoMistyBlue => &MISTY_BLUE,
            Self::NeoSunlitChartreuse => &SUNLIT_CHARTREUSE,
            Self::NeoSunriseGold => &SUNRISE_GOLD,
            Self::NeoDeepVine => &DEEP_VINE,
            Self::NeoDeepForest => &DEEP_FOREST,
            Self::NeoMidnightTeal => &MIDNIGHT_TEAL,
            Self::NeoDeepHarbor => &DEEP_HARBOR,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::NeoMilk => "Neo · Milk",
            Self::NeoPaleMist => "Neo · Pale Mist",
            Self::NeoMistyBlue => "Neo · Misty Blue",
            Self::NeoSunlitChartreuse => "Neo · Sunlit Chartreuse",
            Self::NeoSunriseGold => "Neo · Sunrise Gold",
            Self::NeoDeepVine => "Neo · Deep Vine",
            Self::NeoDeepForest => "Neo · Deep Forest",
            Self::NeoMidnightTeal => "Neo · Midnight Teal",
            Self::NeoDeepHarbor => "Neo · Deep Harbor",
        }
    }

    /// The name a client persists the choice under: the Mac's `TerminalColorTheme` raw value.
    pub fn id(self) -> &'static str {
        match self {
            Self::NeoMilk => "neoMilk",
            Self::NeoPaleMist => "neoPaleMist",
            Self::NeoMistyBlue => "neoMistyBlue",
            Self::NeoSunlitChartreuse => "neoSunlitChartreuse",
            Self::NeoSunriseGold => "neoSunriseGold",
            Self::NeoDeepVine => "neoDeepVine",
            Self::NeoDeepForest => "neoDeepForest",
            Self::NeoMidnightTeal => "neoMidnightTeal",
            Self::NeoDeepHarbor => "neoDeepHarbor",
        }
    }

    /// The theme persisted as `id`; `None` for a name this build does not know.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|theme| theme.id() == id)
    }
}
pub(super) static MILK: Palette = Palette {
    screen: 0xE8EDF4,
    canvas: 0xE0E5EC,
    face: 0xE8EDF4,
    hover: 0xDCE2EA,
    well: 0xD8DEE8,
    ink: 0x3E4A66,
    secondary: 0x6E7A96,
    muted: 0x8E9AB3,
    accent: 0x5B7CFA,
    on_accent: 0xFFFFFF,
    shadow_dark: 0xA6B4C8,
    shadow_light: 0xFFFFFF,
    well_shadow: 0xA6B4C8,
    well_rim: 0xFFFFFF,
    selection: 0xC9D6EE,
    presentation_letterbox: 0x151A24,
    well_lip: None,
    agent_plates: &[0xD8E0FE, 0xCDE7DD, 0xF6D6DB, 0xEEE3CD],
    ansi: [
        0x1F2633, 0xD9556A, 0x2E9A73, 0xB98A2E, 0x3D7DD9, 0x9A6BD0, 0x2E93A6, 0x8E9AB3, 0x6E7A96,
        0xED6F86, 0x3FB68B, 0xE9B04E, 0x5B9DF5, 0xB08CFF, 0x3FB6C8, 0x263043,
    ],
    syntax_type: 0x8E5BFA,
    diff_added: 0x3F9C6A,
};
pub(super) static PALE_MIST: Palette = Palette {
    screen: 0xC0C8CA,
    canvas: 0xB8C0C2,
    face: 0xC0C8CA,
    hover: 0xB2BABC,
    well: 0xAFB7B9,
    ink: 0x25363B,
    secondary: 0x4B6268,
    muted: 0x5E8089,
    accent: 0x0F64D1,
    on_accent: 0xFFFFFF,
    shadow_dark: 0x8C9395,
    shadow_light: 0xD1D9DB,
    well_shadow: 0x8C9395,
    well_rim: 0xD1D9DB,
    selection: 0x99B2CC,
    presentation_letterbox: 0x172226,
    well_lip: None,
    agent_plates: &[0xC3A2D1, 0xE19BA9, 0x94B88A, 0x68BCB3, 0x91AAC4],
    ansi: [
        0x1F2F33, 0xCE2E48, 0x257C5C, 0x8B6823, 0x286CCD, 0x8952C8, 0x257887, 0x4B6268, 0x5E8089,
        0xD71B3D, 0x2B7B5E, 0x946412, 0x0E68E2, 0x7E42FF, 0x267885, 0x25363B,
    ],
    syntax_type: 0x7E42FF,
    diff_added: 0x2B7B5E,
};
pub(super) static MISTY_BLUE: Palette = Palette {
    screen: 0x83A7B4,
    canvas: 0x7B9FAC,
    face: 0x83A7B4,
    hover: 0x769AA7,
    well: 0x7296A2,
    ink: 0x0C1012,
    secondary: 0x37464C,
    muted: 0x4A626B,
    accent: 0x004497,
    on_accent: 0xFFFFFF,
    shadow_dark: 0x517480,
    shadow_light: 0x93B7C5,
    well_shadow: 0x517480,
    well_rim: 0x93B7C5,
    selection: 0x6691AE,
    presentation_letterbox: 0x0B151A,
    well_lip: None,
    agent_plates: &[0x9E7FAB, 0xB97785, 0x719368, 0x41978F, 0x5E89A6],
    ansi: [
        0x1F2E33, 0x9C2336, 0x1C5D46, 0x694E1A, 0x1E519B, 0x6B35A8, 0x1C5A66, 0x37464C, 0x4A626B,
        0xA3142F, 0x205C47, 0x6F4C0E, 0x0A4FAA, 0x4E00FB, 0x1D5A64, 0x0C1012,
    ],
    syntax_type: 0x4E00FB,
    diff_added: 0x205C47,
};
pub(super) static SUNLIT_CHARTREUSE: Palette = Palette {
    screen: 0xDCD870,
    canvas: 0xD2CE67,
    face: 0xDCD870,
    hover: 0xCDCA63,
    well: 0xC6C35C,
    ink: 0x3E3D28,
    secondary: 0x6A684C,
    muted: 0x88865E,
    accent: 0xB55C03,
    on_accent: 0xFFFFFF,
    shadow_dark: 0xA2A13C,
    shadow_light: 0xECE77F,
    well_shadow: 0xA2A13C,
    well_rim: 0xECE77F,
    selection: 0xD3BD58,
    presentation_letterbox: 0x242315,
    well_lip: None,
    agent_plates: &[0xD1AFDE, 0xEEA8B5, 0xA1C497, 0x75C9C0, 0xCAB550],
    ansi: [
        0x33321F, 0xD44158, 0x288664, 0x977026, 0x3275D7, 0x915ECC, 0x298192, 0x6A684C, 0x88865E,
        0xE4274A, 0x2E8566, 0xA06D14, 0x1271F1, 0x854DFF, 0x298290, 0x3E3D28,
    ],
    syntax_type: 0x854DFF,
    diff_added: 0x2E8566,
};
pub(super) static SUNRISE_GOLD: Palette = Palette {
    screen: 0xEDB964,
    canvas: 0xE4B15C,
    face: 0xEDB964,
    hover: 0xDEAB57,
    well: 0xD8A652,
    ink: 0x362E22,
    secondary: 0x645948,
    muted: 0x84755B,
    accent: 0xA84500,
    on_accent: 0xFFFFFF,
    shadow_dark: 0xB38531,
    shadow_light: 0xFFCA73,
    well_shadow: 0xB38531,
    well_rim: 0xFFCA73,
    selection: 0xDE9F4E,
    presentation_letterbox: 0x241D14,
    well_lip: None,
    agent_plates: &[0xBE9ECC, 0xDB96A4, 0x90B386, 0x63B7AE, 0xD59746],
    ansi: [
        0x332B1F, 0xC72D45, 0x247859, 0x876422, 0x2768C7, 0x864DC7, 0x247484, 0x645948, 0x84755B,
        0xD01A3B, 0x29775B, 0x906112, 0x0D65DB, 0x793CFF, 0x257481, 0x362E22,
    ],
    syntax_type: 0x793CFF,
    diff_added: 0x29775B,
};
pub(super) static DEEP_VINE: Palette = Palette {
    screen: 0x3B3D26,
    canvas: 0x32341E,
    face: 0x3B3D26,
    hover: 0x464831,
    well: 0x242711,
    ink: 0xF1F2E9,
    secondary: 0xBFC0AE,
    muted: 0xA1A38C,
    accent: 0xF5C64C,
    on_accent: 0x2C2511,
    shadow_dark: 0x22240F,
    shadow_light: 0x4A4C35,
    well_shadow: 0x131400,
    well_rim: 0x3F412A,
    selection: 0x736631,
    presentation_letterbox: 0x11120B,
    well_lip: Some(0x060700),
    agent_plates: &[0x392840, 0x47232C, 0x1A3915, 0x003531, 0x6B5F2A],
    ansi: [
        0x1D1E12, 0xEF4747, 0x00D9A3, 0xF59E0B, 0x7573FF, 0xEA00E9, 0x00A5B2, 0xBFC0AE, 0xA1A38C,
        0xEF4747, 0x00D9A3, 0xFFAA00, 0x7673FF, 0xEA00E9, 0x00E5E5, 0xF1F2E9,
    ],
    syntax_type: 0xEA00E9,
    diff_added: 0x00D9A3,
};
pub(super) static DEEP_FOREST: Palette = Palette {
    screen: 0x223D22,
    canvas: 0x1A341B,
    face: 0x223D22,
    hover: 0x2E482E,
    well: 0x0D270E,
    ink: 0xE9F2E9,
    secondary: 0xAEC0AE,
    muted: 0x8CA38C,
    accent: 0xB2D353,
    on_accent: 0x252C11,
    shadow_dark: 0x0B240B,
    shadow_light: 0x324C32,
    well_shadow: 0x001200,
    well_rim: 0x264126,
    selection: 0x4D6A31,
    presentation_letterbox: 0x071008,
    well_lip: Some(0x000100),
    agent_plates: &[0x392840, 0x47232C, 0x3A5A33, 0x003531, 0x46632A],
    ansi: [
        0x101C10, 0xEF4444, 0x00D9A3, 0xF59E0B, 0x6F6DFF, 0xE100E0, 0x00A5B2, 0xAEC0AE, 0x8CA38C,
        0xEF4444, 0x00D9A3, 0xFFAA00, 0x706CFF, 0xE500E5, 0x00E5E5, 0xE9F2E9,
    ],
    syntax_type: 0xE500E5,
    diff_added: 0x00D9A3,
};
pub(super) static MIDNIGHT_TEAL: Palette = Palette {
    screen: 0x062635,
    canvas: 0x00202E,
    face: 0x062635,
    hover: 0x133140,
    well: 0x001019,
    ink: 0xE9EFF2,
    secondary: 0xAEBAC0,
    muted: 0x8C9CA3,
    accent: 0x7FA7FF,
    on_accent: 0x001F45,
    shadow_dark: 0x000F18,
    shadow_light: 0x1A3746,
    well_shadow: 0x000104,
    well_rim: 0x0B2A39,
    selection: 0x2A4D72,
    presentation_letterbox: 0x000B10,
    well_lip: Some(0x000104),
    agent_plates: &[0x402D48, 0x4F2831, 0x253920, 0x003C37, 0x22466A],
    ansi: [
        0x010507, 0xEF4444, 0x00D9A3, 0xF59E0B, 0x5452FF, 0xC100C0, 0x00A5B2, 0xAEBAC0, 0x8C9CA3,
        0xEF4444, 0x00D9A3, 0xFFAA00, 0x5651FF, 0xE500E5, 0x00E5E5, 0xE9EFF2,
    ],
    syntax_type: 0xE500E5,
    diff_added: 0x00D9A3,
};
pub(super) static DEEP_HARBOR: Palette = Palette {
    screen: 0x2B4851,
    canvas: 0x223F48,
    face: 0x2B4851,
    hover: 0x37545D,
    well: 0x13313A,
    ink: 0xE9F0F2,
    secondary: 0xAEBCC0,
    muted: 0x8C9DA3,
    accent: 0x19B2FF,
    on_accent: 0x002237,
    shadow_dark: 0x112F37,
    shadow_light: 0x3B5861,
    well_shadow: 0x001E26,
    well_rim: 0x304D56,
    selection: 0x266885,
    presentation_letterbox: 0x0B161A,
    well_lip: Some(0x00151B),
    agent_plates: &[0x3B2942, 0x49252D, 0x21351D, 0x003733, 0x1C617D],
    ansi: [
        0x192A30, 0xF15D5D, 0x00D9A3, 0xF59E0B, 0x8280FF, 0xFB00FA, 0x00A5B2, 0xAEBCC0, 0x8C9DA3,
        0xF15D5D, 0x00D9A3, 0xFFAA00, 0x8380FF, 0xFB00FA, 0x00E5E5, 0xE9F0F2,
    ],
    syntax_type: 0xFB00FA,
    diff_added: 0x00D9A3,
};
pub mod radius {
    pub const COMPACT: f32 = 10.0;
    pub const CONTROL: f32 = 12.0;
    pub const CARD: f32 = 20.0;
    pub const PILL: f32 = 999.0;
}
pub mod space {
    pub const X2: f32 = 2.0;
    pub const X3: f32 = 3.0;
    pub const X4: f32 = 4.0;
    pub const X5: f32 = 5.0;
    pub const X6: f32 = 6.0;
    pub const X7: f32 = 7.0;
    pub const X8: f32 = 8.0;
    pub const X10: f32 = 10.0;
    pub const X12: f32 = 12.0;
    pub const X14: f32 = 14.0;
    pub const X16: f32 = 16.0;
    pub const X18: f32 = 18.0;
}
pub mod type_scale {
    pub const MICRO: f32 = 10.0;
    pub const SMALL: f32 = 11.0;
    pub const CONTROL: f32 = 12.0;
    pub const CAPTION: f32 = 13.0;
    pub const BODY: f32 = 15.0;
    pub const SUBHEADING: f32 = 18.0;
    pub const HEADING: f32 = 20.0;
    pub const TITLE: f32 = 28.0;
}
pub mod pane_shell {
    pub const CORNER_RADIUS: f32 = 20.0;
    pub const SHADOW_OFFSET: f32 = 5.0;
    pub const SHADOW_BLUR: f32 = 10.0;
}
pub mod pane_layout {
    pub const PANE_GAP: f32 = 18.0;
    pub const SCREEN_PADDING: f32 = 18.0;
    pub const GRID_PADDING: f32 = 6.0;
    pub const PANE_CARD_INSET: f32 = 10.0;
    pub const PANE_STACK_SPACING: f32 = 8.0;
    pub const HEADER_HEIGHT: f32 = 26.0;
    pub const HEADER_STATE_DOT: f32 = 8.0;
    pub const HEADER_ACTION_WIDTH: f32 = 24.0;
    pub const HEADER_TICKER_MINIMUM_WIDTH: f32 = 80.0;
    pub const TERMINAL_FONT_SIZE: f32 = 13.0;
    pub const TERMINAL_FONT_SIZE_MINIMUM: f32 = 8.0;
    pub const TERMINAL_FONT_SIZE_MAXIMUM: f32 = 32.0;
    pub const TERMINAL_FONT_SIZE_STEP: f32 = 1.0;
}
pub mod layout {
    pub const RAIL_WIDTH: f32 = 56.0;
    pub const RAIL_BUTTON_SIZE: f32 = 36.0;
    pub const RAIL_DIVIDER_WIDTH: f32 = 24.0;
    pub const TOP_BAR_HEIGHT: f32 = 42.0;
    pub const TOP_BAR_LEADING_INSET: f32 = 52.0;
    pub const TOP_BAR_CONTROL_HEIGHT: f32 = 34.0;
    pub const WORKSPACE_TAB_HEIGHT: f32 = 36.0;
    pub const WORKSPACE_TAB_MAXIMUM_WIDTH: f32 = 216.0;
    pub const WORKSPACE_OVERFLOW_BUTTON_WIDTH: f32 = 44.0;
    pub const WORKSPACE_OVERFLOW_POPOVER_WIDTH: f32 = 340.0;
    pub const WORKSPACE_OVERFLOW_POPOVER_MAXIMUM_HEIGHT: f32 = 420.0;
    pub const READINESS_WIDTH: f32 = 480.0;
    pub const LOOP_PANEL_WIDTH: f32 = 400.0;
    pub const TASK_COLUMN_WIDTH: f32 = 250.0;
    pub const TASK_COLUMN_MIN_HEIGHT: f32 = 500.0;
    pub const TASK_SHEET_WIDTH: f32 = 460.0;
    pub const TASK_SHEET_HEIGHT: f32 = 826.0;
    pub const ATTENTION_POPOVER_WIDTH: f32 = 340.0;
    pub const SWITCH_POPOVER_WIDTH: f32 = 448.0;
    pub const HARNESS_PICKER_WIDTH: f32 = 420.0;
    pub const HARNESS_LIST_HEIGHT: f32 = 260.0;
    pub const SEARCH_FIELD_HEIGHT: f32 = 32.0;
    pub const MODEL_FIELD_HEIGHT: f32 = 30.0;
}
pub mod glyph {
    pub const RAIL: f32 = 18.0;
    pub const TOP_BAR_TOGGLE: f32 = 15.0;
    pub const TOP_BAR_NEW_PANE: f32 = 14.0;
    pub const TAB_CLOSE: f32 = 9.0;
    pub const CHEVRON: f32 = 11.0;
    pub const TASK_TOOLBAR_ICON: f32 = 13.0;
    pub const TASK_TOOLBAR_CHEVRON: f32 = 10.0;
}
pub mod usage_ring {
    pub const DIAMETER: f32 = 40.0;
    pub const FACE_DIAMETER: f32 = 34.0;
    pub const ARC_WIDTH: f32 = 3.0;
    pub const TRACK_WIDTH: f32 = 5.0;
    pub const GLYPH_SIZE: f32 = 16.0;
    pub const PERCENTAGE_SIZE: f32 = 9.0;
    pub const PERCENTAGE_HEIGHT: f32 = 15.0;
    pub const HEIGHT: f32 = 55.0;
}
pub mod pane_interior {
    pub const CARD_RADIUS: f32 = 12.0;
    pub const COMPOSER_RADIUS: f32 = 14.0;
    pub const WELL_RADIUS: f32 = 8.0;
    pub const CARD_PADDING: f32 = 14.0;
    pub const PHONE_INSET: f32 = 16.0;
    pub const CARD_VERTICAL_PADDING: f32 = 12.0;
    pub const STACK_SPACING: f32 = 12.0;
    pub const COMPACT_SPACING: f32 = 8.0;
    pub const TIGHT_SPACING: f32 = 4.0;
    pub const ROW_SPACING: f32 = 10.0;
    pub const ROW_VERTICAL_PADDING: f32 = 8.0;
    pub const SEPARATOR_WIDTH: f32 = 1.0;
    pub const SEND_BUTTON_SIZE: f32 = 28.0;
    pub const PILL_HORIZONTAL_PADDING: f32 = 16.0;
    pub const PILL_VERTICAL_PADDING: f32 = 8.0;
    pub const BODY_POINT_SIZE: f32 = 14.0;
    pub const CAPTION_POINT_SIZE: f32 = 12.0;
    pub const HEADING_POINT_SIZE: f32 = 13.0;
    pub const MONO_POINT_SIZE: f32 = 12.0;
    pub const MONO_SMALL_POINT_SIZE: f32 = 11.0;
    pub const PRESSED_OPACITY: f32 = 0.78;
    pub const DISABLED_OPACITY: f32 = 0.45;
}
pub mod composer {
    pub const ROW_SPACING: f32 = 10.0;
    pub const CONTROL_SPACING: f32 = 4.0;
    pub const CONTROL_HEIGHT: f32 = 28.0;
    pub const CHIP_HORIZONTAL_PADDING: f32 = 8.0;
    pub const CHIP_SPACING: f32 = 5.0;
    pub const ICON_SIZE: f32 = 16.0;
    pub const CHEVRON_SIZE: f32 = 11.0;
    pub const SHIELD_SIZE: f32 = 13.0;
    pub const HARNESS_LOGO_SIZE: f32 = 14.0;
    pub const SEND_WIDTH: f32 = 40.0;
    pub const STOP_SIZE: f32 = 10.0;
    pub const STOP_RADIUS: f32 = 2.0;
    pub const RING_DIAMETER: f32 = 16.0;
    pub const RING_LINE_WIDTH: f32 = 3.0;
    pub const CONTEXT_SPACING: f32 = 6.0;
    pub const CONTEXT_HORIZONTAL_PADDING: f32 = 4.0;
    pub const CHIPS_MINIMUM_WIDTH: f32 = 112.0;
    pub const CONTEXT_PERCENTAGE_WIDTH: f32 = 30.0;
}
pub mod editor {
    pub const FONT_POINT_SIZE: f32 = 11.5;
    pub const LINE_HEIGHT: f32 = 19.0;
    pub const GUTTER_WIDTH: f32 = 44.0;
    pub const TEXT_INSET: f32 = 10.0;
    pub const HEADER_HEIGHT: f32 = 27.0;
    pub const TAB_HEIGHT: f32 = 26.0;
    pub const VIEWPORT_VERTICAL_INSET: f32 = 20.0;
    pub const STATUS_HEIGHT: f32 = 24.0;
    pub const FIND_FIELD_HEIGHT: f32 = 24.0;
    pub const PROJECT_PANEL_WIDTH: f32 = 212.0;
    pub const PREVIEW_MIN_WIDTH: f32 = 260.0;
    pub const PREVIEW_WIDTH: f32 = 360.0;
    pub const PREVIEW_MAX_WIDTH: f32 = 460.0;
    pub const PREVIEW_HEADER_HEIGHT: f32 = 26.0;
}
pub mod desktop {
    pub const WINDOW_WIDTH: f32 = 1280.0;
    pub const WINDOW_HEIGHT: f32 = 800.0;
    pub const WINDOW_MINIMUM_WIDTH: f32 = 640.0;
    pub const WINDOW_MINIMUM_HEIGHT: f32 = 440.0;
    pub const WINDOW_WIDTH_MARGIN: f32 = 64.0;
    pub const WINDOW_HEIGHT_MARGIN: f32 = 120.0;
    pub const TRAFFIC_LIGHT_INSET: f32 = 16.0;
    pub const TAB_CHARACTER_WIDTH: f32 = 7.4;
    pub const MINIMUM_DRAG_WIDTH: f32 = 48.0;
    pub const WINDOW_CONTROL_GLYPH: f32 = 12.0;
    pub const START_FORM_WIDTH: f32 = 640.0;
    pub const START_PROMPT_MIN_HEIGHT: f32 = 96.0;
    pub const RESIZE_EDGE: f32 = 5.0;
    pub const RESIZE_CORNER: f32 = 12.0;
    pub const HEADER_ACTION_GLYPH: f32 = 14.0;
    pub const COMPOSER_GLYPH: f32 = 14.0;
    pub const CHEVRON_GLYPH: f32 = 9.0;
    pub const EMPTY_STATE_GLYPH: f32 = 42.0;
    pub const INLINE_GLYPH: f32 = 12.0;
    pub const CHECK_GLYPH: f32 = 8.0;
    pub const DISCLOSURE_GLYPH: f32 = 10.0;
    pub const STATUS_DOT: f32 = 7.0;
    pub const CHIP_MAX_WIDTH: f32 = 180.0;
    pub const MENU_MIN_WIDTH: f32 = 180.0;
    pub const TOOLTIP_MAX_WIDTH: f32 = 360.0;
    pub const THEME_CARD_WIDTH: f32 = 300.0;
    pub const THEME_CARD_HEIGHT: f32 = 76.0;
    pub const THEME_CARD_SELECTED_BORDER: f32 = 2.0;
    pub const THEME_CHECK_SIZE: f32 = 15.0;
    pub const SWATCH_WIDTH: f32 = 88.0;
    pub const SWATCH_HEIGHT: f32 = 50.0;
    pub const SWATCH_PLATE_LEFT: f32 = 8.0;
    pub const SWATCH_PLATE_WIDTH: f32 = 48.0;
    pub const SWATCH_PLATE_HEIGHT: f32 = 28.0;
    pub const SWATCH_ACCENT_HEIGHT: f32 = 18.0;
    pub const SWATCH_WELL_LEFT: f32 = 59.0;
    pub const SWATCH_WELL_SIZE: f32 = 22.0;
    pub const MENU_CHECK_GLYPH: f32 = 14.0;
    pub const MENU_HARNESS_ROW_HEIGHT: f32 = 42.0;
    pub const PANE_TITLE_MAX_WIDTH: f32 = 260.0;
    pub const RENAME_FIELD_MIN_WIDTH: f32 = 120.0;
    pub const POPUP_GAP: f32 = 4.0;
    pub const FADE_WIDTH: f32 = 28.0;
    pub const PANE_FLOOR_LABEL_CHARACTERS: f32 = 8.0;
    pub const MINIMUM_PANE_HEIGHT: f32 = 120.0;
    pub const WELL_DEPTH: f32 = 3.0;
    pub const HAIRLINE: f32 = 1.0;
    pub const LINE_HEIGHT_UI: f32 = 1.21;
    pub const LINE_HEIGHT_MONO: f32 = 1.32;
    pub const LINE_HEIGHT_CHROME: f32 = 1.364;
    pub const SHADOW_DARK_OFFSET_SCALE: f32 = 0.908;
    pub const SHADOW_DARK_SIGMA_SCALE: f32 = 0.914;
    pub const SHADOW_DARK_OPACITY: f32 = 0.964;
    pub const SHADOW_HALO_OFFSET_SCALE: f32 = 0.434;
    pub const SHADOW_HALO_SIGMA_SCALE: f32 = 0.865;
    pub const SHADOW_HALO_OPACITY: f32 = 0.8;
    pub const SHADOW_LIGHT_OFFSET_SCALE: f32 = 0.964;
    pub const SHADOW_LIGHT_SIGMA_SCALE: f32 = 0.63;
    pub const SUBAGENT_INDENT: f32 = 22.0;
    pub const SUBAGENT_SUMMARY_INDENT: f32 = 40.0;
    pub const PROGRESS_SEGMENT: f32 = 0.3;
    pub const PROGRESS_PERIOD: f32 = 1.4;
    pub const ATTACHMENT_THUMBNAIL: f32 = 26.0;
    pub const ATTACHMENT_THUMBNAIL_RADIUS: f32 = 4.0;
    pub const ATTACHMENT_CHIP_NAME_WIDTH: f32 = 160.0;
    pub const DROP_BORDER: f32 = 2.0;
    pub const DROP_ZONE_OPACITY: f32 = 0.24;
    pub const DRAG_GHOST_OPACITY: f32 = 0.92;
    pub const LOOP_PANEL_CONTENT_WIDTH: f32 = 372.0;
    pub const LOOP_PANEL_TITLE_SIZE: f32 = 14.0;
    pub const LOOP_TOGGLE_WIDTH: f32 = 40.0;
    pub const LOOP_TOGGLE_HEIGHT: f32 = 22.0;
    pub const LOOP_TOGGLE_KNOB: f32 = 18.0;
    pub const LOOP_PROMPT_HEIGHT: f32 = 88.0;
    pub const TASK_DOT: f32 = 6.0;
    pub const NEW_TASK_SHEET_WIDTH: f32 = 560.0;
    pub const TASK_LIST_MINIMUM_WIDTH: f32 = 960.0;
    pub const TASK_LIST_KEY_WIDTH: f32 = 70.0;
    pub const TASK_LIST_STATE_WIDTH: f32 = 120.0;
    pub const TASK_LIST_PRIORITY_WIDTH: f32 = 90.0;
    pub const TASK_LIST_OWNER_WIDTH: f32 = 120.0;
    pub const TASK_LIST_WORKTREE_WIDTH: f32 = 90.0;
    pub const TASK_LIST_COMMENTS_WIDTH: f32 = 40.0;
    pub const TASK_LIST_UPDATED_WIDTH: f32 = 90.0;
    pub const TASK_LIST_TITLE_MINIMUM_WIDTH: f32 = 220.0;
    pub const TASK_EDITOR_HEIGHT: f32 = 96.0;
    pub const TASK_FIELD_HEIGHT: f32 = 40.0;
    pub const TASK_FIELD_TEXT_SIZE: f32 = 14.0;
    pub const TASK_TOOLBAR_HEIGHT: f32 = 34.0;
    pub const TASK_CARD_GLYPH: f32 = 10.0;
    pub const SWITCH_POPOVER_MAXIMUM_HEIGHT: f32 = 780.0;
    pub const SWITCH_POPOVER_PADDING: f32 = 14.0;
    pub const EFFORT_SEGMENT_INSET: f32 = 3.0;
    pub const PREFERENCES_WIDTH: f32 = 1080.0;
    pub const PREFERENCES_HEIGHT: f32 = 700.0;
    pub const PREFERENCES_INSET: f32 = 12.0;
    pub const PREFERENCES_SIDEBAR_WIDTH: f32 = 220.0;
    pub const PREFERENCES_SIDEBAR_ROW_HEIGHT: f32 = 34.0;
    pub const PREFERENCES_SIDE_PADDING: f32 = 22.0;
    pub const PREFERENCES_TITLE_SIZE: f32 = 16.0;
    pub const PREFERENCES_PAGE_TITLE_SIZE: f32 = 18.0;
    pub const PREFERENCES_GLYPH: f32 = 13.0;
    pub const PREFERENCES_ROW_TITLE_SIZE: f32 = 14.0;
    pub const PREFERENCES_CHIP_HEIGHT: f32 = 26.0;
    pub const PREFERENCES_CHIP_GLYPH: f32 = 10.0;
    pub const USAGE_BALLOON_RADIUS: f32 = 14.0;
    pub const HARNESS_ROW_IDENTITY_WIDTH: f32 = 160.0;
    pub const HARNESS_ROW_STATUS_WIDTH: f32 = 118.0;
    pub const HARNESS_ROW_HEIGHT: f32 = 58.0;
    pub const HARNESS_ROW_WIDE_MINIMUM: f32 = 780.0;
    pub const USAGE_PROVIDER_LIST_WIDTH: f32 = 210.0;
    pub const USAGE_PROVIDER_ROW_HEIGHT: f32 = 42.0;
    pub const USAGE_DETAIL_WIDTH: f32 = 250.0;
    pub const USAGE_BAR_HEIGHT: f32 = 6.0;
    pub const USAGE_STATUS_DOT: f32 = 8.0;
    pub const EDITOR_TAB_BAR_HEIGHT: f32 = 34.0;
    pub const EDITOR_TAB_CLOSE_SLOT: f32 = 12.0;
    pub const EDITOR_DIRTY_DOT: f32 = 6.0;
    pub const EDITOR_CARET_WIDTH: f32 = 2.0;
    pub const EDITOR_PROJECT_ROW_HEIGHT: f32 = 22.0;
    pub const EDITOR_PROJECT_INDENT: f32 = 12.0;
    pub const EDITOR_FIND_STATUS_WIDTH: f32 = 72.0;
    pub const EDITOR_THUMB_INSET: f32 = 2.0;
    pub const EDITOR_THUMB_WIDTH: f32 = 5.0;
    pub const EDITOR_THUMB_MINIMUM: f32 = 24.0;
    pub const EDITOR_MARGIN_CELLS: f32 = 2.0;
    pub const WRITING_TEXT_SIZE: f32 = 17.0;
    pub const WRITING_LINE_HEIGHT: f32 = 1.65;
    pub const WRITING_COLUMN_WIDTH: f32 = 680.0;
    pub const WRITING_TOOLBAR_HEIGHT: f32 = 32.0;
    pub const WRITING_TOOLBAR_BUTTON_WIDTH: f32 = 28.0;
    pub const WRITING_LIST_WIDTH: f32 = 760.0;
    pub const WRITING_ROW_ICON_BOX: f32 = 34.0;
    pub const SCREENPLAY_TEXT_SIZE: f32 = 16.0;
    pub const SCREENPLAY_LINE_HEIGHT: f32 = 27.0;
    pub const SCREENPLAY_PAGE_WIDTH: f32 = 734.0;
    pub const SCREENPLAY_PAGE_INSET_SHARE: f32 = 0.1008;
    pub const SCREENPLAY_DIALOGUE_SHARE: f32 = 0.2048;
    pub const SCREENPLAY_PARAGRAPH_SPACING: f32 = 8.0;
    pub const PRESENTATION_RAIL_WIDTH: f32 = 236.0;
    pub const PRESENTATION_NUMBER_STRIP_HEIGHT: f32 = 32.0;
    pub const PRESENTATION_FOOTER_HEIGHT: f32 = 28.0;
    pub const PRESENTATION_INSET: f32 = 10.0;
    pub const PRESENTATION_RAIL_DIVIDER: f32 = 1.0;
    pub const PRESENTATION_MINIMUM_STAGE_WIDTH: f32 = 320.0;
    pub const PRESENTATION_MINIMUM_STAGE_HEIGHT: f32 = 180.0;
    pub const PRESENTATION_MAXIMUM_STAGE_WIDTH: f32 = 1024.0;
    pub const PRESENTATION_MAXIMUM_STAGE_HEIGHT: f32 = 576.0;
    pub const PRESENTATION_LINE_HEIGHT: f32 = 23.0;
    pub const PRESENTATION_SOURCE_INSET_X: f32 = 48.0;
    pub const PRESENTATION_SOURCE_INSET_Y: f32 = 36.0;
    pub const PRESENTATION_NUMBER_DISC: f32 = 22.0;
    pub const PRESENTATION_THUMBNAIL_HEIGHT: f32 = 100.0;
    pub const PRESENTATION_SELECTED_BORDER: f32 = 2.0;
}
pub mod elevation {
    /// An outer shadow pair of a raised surface: light up and left, dark down and right.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Elevation {
        pub light_offset: f32,
        pub light_blur: f32,
        pub dark_offset: f32,
        pub dark_blur: f32,
    }
    pub const CHIP: Elevation = Elevation {
        light_offset: 3.0,
        light_blur: 5.0,
        dark_offset: 3.0,
        dark_blur: 5.0,
    };
    pub const TOGGLE: Elevation = Elevation {
        light_offset: 2.0,
        light_blur: 4.0,
        dark_offset: 2.0,
        dark_blur: 4.0,
    };
    pub const TASK_CARD: Elevation = Elevation {
        light_offset: 3.0,
        light_blur: 6.0,
        dark_offset: 3.0,
        dark_blur: 6.0,
    };
    pub const ACTION: Elevation = Elevation {
        light_offset: 4.0,
        light_blur: 8.0,
        dark_offset: 4.0,
        dark_blur: 8.0,
    };
    pub const CARD: Elevation = Elevation {
        light_offset: 4.0,
        light_blur: 9.0,
        dark_offset: 4.0,
        dark_blur: 9.0,
    };
    pub const PILL: Elevation = Elevation {
        light_offset: 6.0,
        light_blur: 10.0,
        dark_offset: 6.0,
        dark_blur: 10.0,
    };
    pub const PANEL: Elevation = Elevation {
        light_offset: 9.0,
        light_blur: 18.0,
        dark_offset: 9.0,
        dark_blur: 18.0,
    };
    pub const BALLOON: Elevation = Elevation {
        light_offset: 4.0,
        light_blur: 10.0,
        dark_offset: 6.0,
        dark_blur: 14.0,
    };
}
pub mod fonts {
    pub const CHROME: &str = "Nunito";
    pub const UI: &str = "Inter";
    pub const MONO: &str = "JetBrains Mono";
    pub const SCREENPLAY: &str = "Courier Prime";
}
