//! A terminal pane as the view paints it: styled rows of the screen the desktop client keeps, and
//! the keys that go back.

use soyeht_client::{
    keys::Modifiers,
    screen::{CellColor, DIM_OPACITY, Run, TerminalScreen, xterm_color},
};

#[derive(uniffi::Enum)]
pub enum TermColor {
    DefaultForeground,
    DefaultBackground,
    /// One of the sixteen ANSI slots, which the view takes from the theme.
    Indexed {
        index: u8,
    },
    Rgb {
        red: u8,
        green: u8,
        blue: u8,
    },
}

#[derive(uniffi::Record)]
pub struct TermRun {
    pub column: u32,
    pub columns: u32,
    pub text: String,
    pub foreground: TermColor,
    pub background: TermColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
}

#[derive(uniffi::Record)]
pub struct TermCursor {
    pub row: u32,
    pub column: u32,
    /// The character under the cursor, which the view draws over it in the on-accent color.
    pub glyph: String,
}

#[derive(uniffi::Record)]
pub struct TerminalFrame {
    /// How much of its opacity dim text loses: every client paints it the same.
    pub dim_opacity: f32,
    pub rows: u32,
    pub columns: u32,
    pub lines: Vec<Vec<TermRun>>,
    pub cursor: Option<TermCursor>,
}

fn color(color: CellColor) -> TermColor {
    match color {
        CellColor::DefaultFg => TermColor::DefaultForeground,
        CellColor::DefaultBg => TermColor::DefaultBackground,
        // The sixteen ANSI slots are the theme's; the cube and the grays are xterm's, which every
        // client shares (`screen::xterm_color`).
        CellColor::Indexed(index) => match xterm_color(index) {
            Some(rgb) => TermColor::Rgb {
                red: (rgb >> 16) as u8,
                green: (rgb >> 8) as u8,
                blue: rgb as u8,
            },
            None => TermColor::Indexed { index },
        },
        CellColor::Rgb(red, green, blue) => TermColor::Rgb { red, green, blue },
    }
}

fn run(run: Run) -> TermRun {
    TermRun {
        column: run.column.into(),
        columns: run.columns.into(),
        text: run.text,
        foreground: color(run.fg),
        background: color(run.bg),
        bold: run.bold,
        italic: run.italic,
        underline: run.underline,
        dim: run.dim,
    }
}

pub(crate) fn frame(screen: &TerminalScreen) -> TerminalFrame {
    let (rows, columns) = screen.size();
    TerminalFrame {
        dim_opacity: DIM_OPACITY,
        rows: rows.into(),
        columns: columns.into(),
        lines: (0..rows)
            .map(|row| {
                screen
                    .row_runs_within(row, u16::MAX)
                    .into_iter()
                    .map(run)
                    .collect()
            })
            .collect(),
        cursor: screen.cursor().map(|cursor| TermCursor {
            row: cursor.row.into(),
            column: cursor.column.into(),
            glyph: screen.text_at(cursor.row, cursor.column),
        }),
    }
}

/// A key that is not text, which the soft keyboard's extra row or a hardware keyboard sends.
#[derive(uniffi::Enum)]
pub enum TerminalKey {
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    /// A character with Control or Alt held (Control-C): the text it types.
    Character {
        text: String,
    },
}

impl TerminalKey {
    /// The name the client's key encoder knows the key by (`soyeht_client::keys`).
    fn name(&self) -> &str {
        match self {
            Self::Enter => "enter",
            Self::Backspace => "backspace",
            Self::Tab => "tab",
            Self::Escape => "escape",
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Character { text } => text,
        }
    }
}

#[derive(uniffi::Record)]
pub struct KeyPress {
    pub key: TerminalKey,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The bytes a key press sends, or none for a key a terminal has no byte for.
pub(crate) fn encode(press: &KeyPress, screen: &TerminalScreen) -> Option<Vec<u8>> {
    let modifiers = Modifiers {
        control: press.control,
        alt: press.alt,
        shift: press.shift,
    };
    soyeht_client::keys::encode(
        press.key.name(),
        None,
        modifiers,
        screen.application_cursor(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: TerminalKey, control: bool) -> KeyPress {
        KeyPress {
            key,
            control,
            alt: false,
            shift: false,
        }
    }

    #[test]
    fn the_keys_are_encoded_as_the_desktop_client_encodes_them() {
        let screen = TerminalScreen::new(24, 80);
        assert_eq!(
            encode(&press(TerminalKey::Enter, false), &screen),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode(&press(TerminalKey::Escape, false), &screen),
            Some(b"\x1b".to_vec())
        );
        assert_eq!(
            encode(&press(TerminalKey::Up, false), &screen),
            Some(b"\x1b[A".to_vec())
        );
        let control_c = press(
            TerminalKey::Character {
                text: "c".to_owned(),
            },
            true,
        );
        assert_eq!(encode(&control_c, &screen), Some(vec![3]));
    }

    #[test]
    fn xterm_colors_are_resolved_here_and_the_ansi_slots_are_left_to_the_theme() {
        assert!(matches!(
            color(CellColor::Indexed(5)),
            TermColor::Indexed { index: 5 }
        ));
        assert!(matches!(
            color(CellColor::Indexed(196)),
            TermColor::Rgb {
                red: 255,
                green: 0,
                blue: 0
            }
        ));
        assert!(matches!(
            color(CellColor::Indexed(232)),
            TermColor::Rgb {
                red: 8,
                green: 8,
                blue: 8
            }
        ));
    }

    #[test]
    fn a_frame_carries_the_cursor_glyph_and_the_dim_opacity() {
        let mut screen = TerminalScreen::new(4, 10);
        screen.feed(b"hi\x1b[1D");
        let frame = frame(&screen);
        assert_eq!(frame.dim_opacity, DIM_OPACITY);
        assert_eq!(
            frame.cursor.map(|cursor| cursor.glyph),
            Some("i".to_owned())
        );
        assert_eq!(frame.lines.len(), 4);
    }
}
