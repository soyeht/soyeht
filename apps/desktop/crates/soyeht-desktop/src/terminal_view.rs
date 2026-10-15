//! Paints a terminal screen: backgrounds, text and the cursor, on the character grid.

use crate::{
    theme::{Palette, desktop, fonts::MONO as MONO_FAMILY, pane_layout::TERMINAL_FONT_SIZE},
    workbench::Workbench,
};
use gpui::{
    App, BorderStyle, Bounds, Entity, Hsla, IntoElement, Pixels, Styled, TextAlign, TextRun,
    UnderlineStyle, Window, canvas, fill, font, outline, point, px, rgb, size,
};
use soyeht_client::screen::{CellColor, Cursor, Run};

/// Dim text keeps its hue and loses this much opacity.
const DIM_OPACITY: f32 = 0.7;

/// The size of one character cell.
#[derive(Clone, Copy)]
pub struct CellSize {
    pub width: Pixels,
    pub height: Pixels,
}

impl CellSize {
    pub fn measure(cx: &App) -> Self {
        let text = cx.text_system();
        let size = px(TERMINAL_FONT_SIZE);
        let id = text.resolve_font(&font(MONO_FAMILY));
        let width = text
            .advance(id, size, 'm')
            .map_or(size * 0.6, |advance| advance.width);
        Self {
            width,
            height: px((TERMINAL_FONT_SIZE * desktop::LINE_HEIGHT_MONO).round()),
        }
    }

    /// `(rows, columns)` that fit in `bounds`, never fewer than one of each.
    pub fn grid(self, bounds: Bounds<Pixels>) -> (u16, u16) {
        let fit = |extent: Pixels, cell: Pixels| ((extent / cell).floor().max(1.0)) as u16;
        (
            fit(bounds.size.height, self.height),
            fit(bounds.size.width, self.width),
        )
    }
}

/// What a frame needs from the screen, copied out so painting holds no borrow of the app.
struct Frame {
    cell: CellSize,
    rows: Vec<Vec<Run>>,
    cursor: Option<(Cursor, String)>,
}

/// A canvas that shows `pane_id`'s screen and keeps the screen fitted to its own size.
pub fn surface(
    workbench: Entity<Workbench>,
    pane_id: String,
    palette: &'static Palette,
    focused: bool,
) -> impl IntoElement {
    canvas(
        move |bounds, _window, cx| {
            let cell = CellSize::measure(cx);
            let (rows, cols) = cell.grid(bounds);
            workbench.update(cx, |workbench, _| {
                workbench.fit_terminal(&pane_id, rows, cols)
            });
            let workbench = workbench.read(cx);
            let screen = workbench.screen(&pane_id)?;
            let (shown_rows, _) = screen.size();
            Some(Frame {
                cell,
                rows: (0..shown_rows).map(|row| screen.row_runs(row)).collect(),
                cursor: screen
                    .cursor()
                    .map(|cursor| (cursor, screen.text_at(cursor.row, cursor.column))),
            })
        },
        move |bounds, frame, window, cx| {
            if let Some(frame) = frame {
                paint(&frame, bounds, palette, focused, window, cx);
            }
        },
    )
    .size_full()
}

fn paint(
    frame: &Frame,
    bounds: Bounds<Pixels>,
    palette: &Palette,
    focused: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let cell = frame.cell;
    let origin = bounds.origin;
    for (row, runs) in frame.rows.iter().enumerate() {
        let y = origin.y + cell.height * row as f32;
        for run in runs {
            let x = origin.x + cell.width * f32::from(run.column);
            if run.bg != CellColor::DefaultBg {
                let area = Bounds::new(
                    point(x, y),
                    size(cell.width * f32::from(run.columns), cell.height),
                );
                window.paint_quad(fill(area, rgb(palette.terminal(run.bg))));
            }
            if run.text.trim().is_empty() && !run.underline {
                continue;
            }
            draw_text(
                &run.text,
                run_style(run, palette),
                point(x, y),
                cell,
                window,
                cx,
            );
        }
    }
    if let Some((cursor, glyph)) = &frame.cursor {
        let x = origin.x + cell.width * f32::from(cursor.column);
        let y = origin.y + cell.height * f32::from(cursor.row);
        let area = Bounds::new(point(x, y), size(cell.width, cell.height));
        if focused {
            window.paint_quad(fill(area, rgb(palette.accent)));
            let style = TextStyle {
                color: rgb(palette.on_accent).into(),
                ..TextStyle::default()
            };
            draw_text(glyph, style, point(x, y), cell, window, cx);
        } else {
            window.paint_quad(outline(area, rgb(palette.accent), BorderStyle::Solid));
        }
    }
}

#[derive(Default)]
struct TextStyle {
    color: Hsla,
    bold: bool,
    italic: bool,
    underline: bool,
}

fn run_style(run: &Run, palette: &Palette) -> TextStyle {
    let mut color: Hsla = rgb(palette.terminal(run.fg)).into();
    if run.dim {
        color.a *= DIM_OPACITY;
    }
    TextStyle {
        color,
        bold: run.bold,
        italic: run.italic,
        underline: run.underline,
    }
}

fn draw_text(
    text: &str,
    style: TextStyle,
    at: gpui::Point<Pixels>,
    cell: CellSize,
    window: &mut Window,
    cx: &mut App,
) {
    let mut face = font(MONO_FAMILY);
    if style.bold {
        face = face.bold();
    }
    if style.italic {
        face = face.italic();
    }
    let run = TextRun {
        len: text.len(),
        font: face,
        color: style.color,
        background_color: None,
        underline: style.underline.then_some(UnderlineStyle {
            thickness: px(desktop::HAIRLINE),
            color: Some(style.color),
            wavy: false,
        }),
        strikethrough: None,
    };
    let shaped = window.text_system().shape_line(
        text.to_owned().into(),
        px(TERMINAL_FONT_SIZE),
        &[run],
        Some(cell.width),
    );
    // A run that fails to paint leaves its cells blank until the screen next changes.
    if let Err(error) = shaped.paint(at, cell.height, TextAlign::Left, None, window, cx) {
        eprintln!("soyeht: could not paint terminal text: {error}");
    }
}
