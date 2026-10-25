//! Paints a terminal screen: backgrounds, text and the cursor, on the character grid.

use crate::{
    theme::{Palette, desktop, fonts::MONO as MONO_FAMILY, pane_layout::TERMINAL_FONT_SIZE},
    workbench::Workbench,
};
use gpui::{
    App, BorderStyle, Bounds, DispatchPhase, Entity, Hsla, IntoElement, MouseMoveEvent,
    MouseUpEvent, Pixels, ShapedLine, Styled, TextAlign, TextRun, UnderlineStyle, Window, canvas,
    fill, font, outline, point, px, rgb, size,
};
use soyeht_client::screen::{CellColor, Cursor, Run};
use std::sync::atomic::{AtomicBool, Ordering};

/// Dim text keeps its hue and loses this much opacity.
const DIM_OPACITY: f32 = 0.7;

/// Whether the last run to paint failed: a failure is logged when it starts, not on every frame.
static PAINT_FAILING: AtomicBool = AtomicBool::new(false);

/// The size of one character cell.
#[derive(Clone, Copy)]
pub struct CellSize {
    pub width: Pixels,
    pub height: Pixels,
}

impl CellSize {
    pub fn measure(cx: &App) -> Self {
        Self::of(
            cx,
            TERMINAL_FONT_SIZE,
            (TERMINAL_FONT_SIZE * desktop::LINE_HEIGHT_MONO).round(),
        )
    }

    /// The cell of the mono face at `font_size` points, on a line `height` points tall.
    pub fn of(cx: &App, font_size: f32, height: f32) -> Self {
        let text = cx.text_system();
        let size = px(font_size);
        let id = text.resolve_font(&font(MONO_FAMILY));
        let width = text
            .advance(id, size, 'm')
            .map_or(size * 0.6, |advance| advance.width);
        Self {
            width,
            height: px(height),
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
    /// For each row, the columns that are selected, and the ones of the address under the
    /// pointer: from the first to one past the last.
    selected: Vec<Option<(u16, u16)>>,
    link: Vec<Option<(u16, u16)>>,
    /// A drag that selects is under way, so the pointer is followed wherever it goes.
    dragging: bool,
    /// The text an input method is composing, drawn at the cursor until it is committed.
    preedit: Option<(String, Cursor)>,
}

/// The columns of a row that are inside a view `columns` wide.
fn within(span: Option<(u16, u16)>, columns: u16) -> Option<(u16, u16)> {
    span.filter(|(from, _)| *from < columns)
        .map(|(from, to)| (from, to.min(columns)))
}

/// A canvas that gathers what a frame draws from the app, then draws it. `gather` runs while the
/// frame is laid out and may change the app; `draw` only paints.
pub fn frame_canvas<F: 'static>(
    gather: impl Fn(Bounds<Pixels>, &mut App) -> Option<F> + 'static,
    draw: impl Fn(&F, Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    canvas(
        move |bounds, _window, cx| gather(bounds, cx),
        move |bounds, frame, window, cx| {
            if let Some(frame) = frame {
                draw(&frame, bounds, window, cx);
            }
        },
    )
    .size_full()
}

/// A canvas that shows `pane_id`'s screen and keeps the screen fitted to its own size.
pub fn surface(
    workbench: Entity<Workbench>,
    pane_id: String,
    palette: &'static Palette,
    focused: bool,
) -> impl IntoElement {
    let (drag_workbench, drag_pane) = (workbench.clone(), pane_id.clone());
    frame_canvas(
        move |bounds, cx| {
            let cell = CellSize::measure(cx);
            let (rows, cols) = cell.grid(bounds);
            workbench.update(cx, |workbench, cx| {
                workbench.fit_terminal(&pane_id, (rows, cols), cx);
                workbench.record_terminal_geometry(&pane_id, bounds, cell);
            });
            let workbench = workbench.read(cx);
            let screen = workbench.screen(&pane_id)?;
            // While the screen waits for the size to settle it can be larger than the view: the
            // view shows what fits.
            let shown_rows = screen.size().0.min(rows);
            let hovered = workbench.hovered_link(&pane_id);
            Some(Frame {
                cell,
                rows: (0..shown_rows)
                    .map(|row| screen.row_runs_within(row, cols))
                    .collect(),
                cursor: screen
                    .cursor()
                    .filter(|cursor| cursor.row < rows && cursor.column < cols)
                    .map(|cursor| (cursor, screen.text_at(cursor.row, cursor.column))),
                selected: (0..shown_rows)
                    .map(|row| within(screen.selection_columns(row), cols))
                    .collect(),
                link: (0..shown_rows)
                    .map(|row| within(hovered.and_then(|span| screen.columns_of(span, row)), cols))
                    .collect(),
                dragging: workbench.terminal_dragging(&pane_id),
                preedit: workbench
                    .preedit_of(&pane_id)
                    .filter(|_| focused)
                    .map(|text| (text.to_owned(), screen.cursor_cell()))
                    .filter(|(_, at)| at.row < rows && at.column < cols),
            })
        },
        move |frame, bounds, window, cx| {
            if frame.dragging {
                follow_drag(&drag_workbench, &drag_pane, window);
            }
            paint(frame, bounds, palette, focused, window, cx);
        },
    )
}

/// While a selection is dragged the pointer is followed over the whole window, not only over the
/// screen: past an edge the drag scrolls, and the button released anywhere ends it.
fn follow_drag(workbench: &Entity<Workbench>, pane_id: &str, window: &mut Window) {
    let (moved, pane) = (workbench.clone(), pane_id.to_owned());
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            moved.update(cx, |workbench, cx| {
                workbench.terminal_drag(&pane, event, cx)
            });
        }
    });
    let (released, pane) = (workbench.clone(), pane_id.to_owned());
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            released.update(cx, |workbench, cx| workbench.terminal_release(&pane, cx));
        }
    });
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
            if run.bg != CellColor::DefaultBg {
                let x = origin.x + cell.width * f32::from(run.column);
                let area = Bounds::new(
                    point(x, y),
                    size(cell.width * f32::from(run.columns), cell.height),
                );
                window.paint_quad(fill(area, rgb(palette.terminal(run.bg))));
            }
        }
        // The selection is under the text, over the cells' own backgrounds.
        if let Some(Some((from, to))) = frame.selected.get(row) {
            let area = Bounds::new(
                point(origin.x + cell.width * f32::from(*from), y),
                size(cell.width * f32::from(to - from), cell.height),
            );
            window.paint_quad(fill(area, rgb(palette.selection)));
        }
        for run in runs {
            let x = origin.x + cell.width * f32::from(run.column);
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
        // The address under the pointer, while the key that opens it is down.
        if let Some(Some((from, to))) = frame.link.get(row) {
            let thickness = px(desktop::HAIRLINE);
            let area = Bounds::new(
                point(
                    origin.x + cell.width * f32::from(*from),
                    y + cell.height - thickness,
                ),
                size(cell.width * f32::from(to - from), thickness),
            );
            window.paint_quad(fill(area, rgb(palette.accent)));
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
    // The composition goes over the cursor, and where the cursor would be when it is hidden.
    if let Some((text, at)) = &frame.preedit {
        let origin = point(
            origin.x + cell.width * f32::from(at.column),
            origin.y + cell.height * f32::from(at.row),
        );
        let ink = palette.terminal(CellColor::DefaultFg);
        let background = palette.terminal(CellColor::DefaultBg);
        paint_preedit(
            text,
            (origin, cell),
            (TERMINAL_FONT_SIZE, ink, background),
            (window, cx),
        );
    }
}

#[derive(Default)]
struct TextStyle {
    color: Hsla,
    bold: bool,
    italic: bool,
    underline: bool,
}

impl TextStyle {
    /// A run of `len` bytes of the mono face in this style.
    fn run(&self, len: usize) -> TextRun {
        let mut face = font(MONO_FAMILY);
        if self.bold {
            face = face.bold();
        }
        if self.italic {
            face = face.italic();
        }
        TextRun {
            len,
            font: face,
            color: self.color,
            background_color: None,
            underline: self.underline.then_some(UnderlineStyle {
                thickness: px(desktop::HAIRLINE),
                color: Some(self.color),
                wavy: false,
            }),
            strikethrough: None,
        }
    }
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
    paint_runs(
        text,
        &[style.run(text.len())],
        TERMINAL_FONT_SIZE,
        at,
        Layout::Grid(cell),
        window,
        cx,
    );
}

/// How the characters of a run of text are laid out.
#[derive(Clone, Copy)]
pub enum Layout {
    /// Each character on its own cell.
    Grid(CellSize),
    /// Each character as wide as its glyphs, on a line of this height: a wide character, or a
    /// letter with its accents.
    Natural(Pixels),
}

/// Shapes `text` in `runs` of the mono face and paints it from `at`.
pub fn paint_runs(
    text: &str,
    runs: &[TextRun],
    font_size: f32,
    at: gpui::Point<Pixels>,
    layout: Layout,
    window: &mut Window,
    cx: &mut App,
) {
    let (force, height) = match layout {
        Layout::Grid(cell) => (Some(cell.width), cell.height),
        Layout::Natural(height) => (None, height),
    };
    let shaped = shape(text, (font_size, runs, force), window);
    paint_line(&shaped, (at, height), window, cx);
}

/// `text` shaped in `runs` of the mono face, each character `force` wide when that is given.
fn shape(
    text: &str,
    (font_size, runs, force): (f32, &[TextRun], Option<Pixels>),
    window: &mut Window,
) -> ShapedLine {
    let text = text.to_owned().into();
    window
        .text_system()
        .shape_line(text, px(font_size), runs, force)
}

/// Paints a shaped line from `at` on a line `height` tall. A run that fails to paint leaves its
/// cells blank until the next frame draws them.
fn paint_line(
    shaped: &ShapedLine,
    (at, height): (gpui::Point<Pixels>, Pixels),
    window: &mut Window,
    cx: &mut App,
) {
    report_paint(
        shaped
            .paint(at, height, TextAlign::Left, None, window, cx)
            .is_err(),
    );
}

/// The text an input method is composing, painted underlined from `at` over what is there: in
/// `(font_size, ink, background)`, each character as wide as its glyphs.
pub fn paint_preedit(
    text: &str,
    (at, cell): (gpui::Point<Pixels>, CellSize),
    (font_size, ink, background): (f32, u32, u32),
    (window, cx): (&mut Window, &mut App),
) {
    let run = TextStyle {
        color: rgb(ink).into(),
        underline: true,
        ..TextStyle::default()
    }
    .run(text.len());
    let shaped = shape(text, (font_size, &[run], None), window);
    window.paint_quad(fill(
        Bounds::new(at, size(shaped.width, cell.height)),
        rgb(background),
    ));
    paint_line(&shaped, (at, cell.height), window, cx);
}

/// Records whether a run of text painted. A failure that lasts is logged once, not on every
/// frame: a paint that works ends the streak.
pub fn report_paint(failed: bool) {
    if !failed {
        PAINT_FAILING.store(false, Ordering::Relaxed);
    } else if !PAINT_FAILING.swap(true, Ordering::Relaxed) {
        soyeht_platform::log::event("desktop.text_paint_failed", &[]);
    }
}
