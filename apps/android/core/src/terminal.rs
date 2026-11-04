//! A terminal pane as the view paints it: styled rows of the screen the desktop client keeps, and
//! the keys that go back.

use soyeht_client::{
    keys::Modifiers,
    screen::{CellColor, Run, TerminalScreen},
};

#[derive(uniffi::Enum)]
pub enum TermColor {
    DefaultForeground,
    DefaultBackground,
    /// One of the 256 indexed colors; the view takes the first 16 from the theme.
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
}

#[derive(uniffi::Record)]
pub struct TerminalFrame {
    pub rows: u32,
    pub columns: u32,
    pub lines: Vec<Vec<TermRun>>,
    pub cursor: Option<TermCursor>,
}

fn color(color: CellColor) -> TermColor {
    match color {
        CellColor::DefaultFg => TermColor::DefaultForeground,
        CellColor::DefaultBg => TermColor::DefaultBackground,
        CellColor::Indexed(index) => TermColor::Indexed { index },
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
        }),
    }
}

/// A key the soft keyboard's extra row or a hardware keyboard sent: its name as the desktop client
/// names keys (`"enter"`, `"up"`, `"f5"`), and the modifiers held.
#[derive(uniffi::Record)]
pub struct KeyPress {
    pub key: String,
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
    soyeht_client::keys::encode(&press.key, None, modifiers, screen.application_cursor())
}
