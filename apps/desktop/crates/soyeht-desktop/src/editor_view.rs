//! Paints the lines of an editor in view: the gutter of line numbers, the text with the color of
//! its syntax, the selection, the matches of a search and the caret. Only the lines in view are
//! shaped and drawn, so a file of any length costs a window's worth of text.

use crate::{
    terminal_view::{CellSize, Layout, frame_canvas, paint_runs},
    theme::{Palette, desktop, editor, fonts::MONO as MONO_FAMILY, type_scale},
    workbench::Workbench,
};
use gpui::{
    App, Bounds, ContentMask, Corners, Entity, Hsla, IntoElement, Pixels, TextAlign, TextRun,
    Window, fill, font, point, px, rgb, size,
};
use soyeht_client::editor::{
    page::Page,
    selection::Selection,
    text::{self, Shown, cells_before, line_cells},
};
use soyeht_protocol::{HighlightKind, TextRange, client_policy::HighlightRole};

/// One line in view, ready to draw.
pub struct Row {
    /// The number the gutter shows, from 1.
    pub number: u32,
    pub shown: Shown,
    /// The cells (from the first one shown) a selection covers, and the matches of a search.
    pub selected: Option<(usize, usize)>,
    pub matches: Vec<(usize, usize)>,
}

/// What a frame draws, copied out of the replica so painting holds no borrow of the app.
pub struct Frame {
    pub cell: CellSize,
    pub rows: Vec<Row>,
    /// The caret, as the row it is on and its cell counted from the line's start, when it is in
    /// view.
    pub caret: Option<(usize, usize)>,
    /// How much of the whole text is scrolled past, and how much is in view: the scrollbar.
    pub scroll: (f32, f32),
    pub content: f32,
    /// The first cell shown on each row, and how far the first row is scrolled up.
    pub first_cell: usize,
    pub offset: (f32, f32),
}

/// The color of the syntax role.
fn role_color(palette: &Palette, role: HighlightRole) -> u32 {
    match role {
        HighlightRole::Accent => palette.accent,
        HighlightRole::SyntaxType => palette.syntax_type,
        HighlightRole::Warning => palette.warning(),
        HighlightRole::Muted => palette.muted,
        HighlightRole::Success => palette.success(),
        HighlightRole::Ink => palette.ink,
    }
}

pub fn kind_color(palette: &Palette, kind: Option<&HighlightKind>) -> u32 {
    kind.map_or(palette.ink, |kind| role_color(palette, kind.role()))
}

/// The cells of line `line` that the range `from..to` covers, when it covers any; a range that
/// goes on to the next line covers the line's end and one cell more, the line break.
fn covered(page: &Page, line: u32, range: &TextRange, first: usize) -> Option<(usize, usize)> {
    let (start, end) = (&range.start, &range.end);
    if line < start.line || line > end.line {
        return None;
    }
    let text = page.text(line);
    let from = if line == start.line {
        cells_before(text, start.column)
    } else {
        0
    };
    let to = if line == end.line {
        cells_before(text, end.column)
    } else {
        line_cells(text) + 1
    };
    (to > from).then(|| (from.saturating_sub(first), to.saturating_sub(first)))
}

/// The rows of the lines in view, from `first_line` for `count` lines of a document of `total`.
/// A line the page does not hold is a blank row, so that the lines that are loaded stay where
/// they are in the document.
pub fn rows(
    page: &Page,
    selection: &Selection,
    matches: &[TextRange],
    (first_line, count, total): (u32, u32, u32),
    (first_cell, columns): (usize, usize),
) -> Vec<Row> {
    let range = selection.range();
    (first_line..first_line.saturating_add(count).min(total))
        .map(|line| {
            let Some(source) = page.line(line) else {
                return Row {
                    number: line + 1,
                    shown: Shown::default(),
                    selected: None,
                    matches: Vec::new(),
                };
            };
            Row {
                number: line + 1,
                shown: text::show(&source.text, &source.highlights, first_cell, columns),
                selected: (!selection.is_empty())
                    .then(|| covered(page, line, &range, first_cell))
                    .flatten(),
                matches: matches
                    .iter()
                    .filter_map(|found| covered(page, line, found, first_cell))
                    .collect(),
            }
        })
        .collect()
}

/// A canvas that shows the lines of `pane_id`'s file and keeps the replica told what is in view.
pub fn surface(
    pane_id: String,
    focused: bool,
    workbench: Entity<Workbench>,
    palette: &'static Palette,
) -> impl IntoElement {
    frame_canvas(
        move |bounds, cx| {
            let cell = CellSize::of(cx, editor::FONT_POINT_SIZE, editor::LINE_HEIGHT);
            workbench.update(cx, |workbench, cx| {
                workbench.editor_frame(&pane_id, bounds, cell, cx)
            })
        },
        move |frame, bounds, window, cx| draw(frame, bounds, window, cx, palette, focused),
    )
}

fn color(value: u32) -> Hsla {
    rgb(value).into()
}

/// Draws the gutter, the lines and the scrollbar of one frame.
fn draw(
    frame: &Frame,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
    palette: &Palette,
    focused: bool,
) {
    let cell = frame.cell;
    let gutter = px(editor::GUTTER_WIDTH);
    let text_area = Bounds::new(
        point(bounds.left() + gutter, bounds.top()),
        size(bounds.size.width - gutter, bounds.size.height),
    );
    let origin = point(
        text_area.left() + px(editor::TEXT_INSET),
        bounds.top() + px(editor::VIEWPORT_VERTICAL_INSET / 2.0) - px(frame.offset.1),
    );
    window.with_content_mask(Some(ContentMask { bounds: text_area }), |window| {
        // The x of the first cell shown: the text is scrolled sideways by whole cells and a bit.
        let first = origin.x - px(frame.offset.0) + cell.width * frame.first_cell as f32;
        for (index, row) in frame.rows.iter().enumerate() {
            let y = origin.y + cell.height * index as f32;
            shade(window, (first, y), cell, &row.matches, palette.well);
            if let Some(selected) = row.selected {
                shade(window, (first, y), cell, &[selected], palette.selection);
            }
            draw_line(&row.shown, first, y, cell, palette, (window, cx));
        }
        if focused && let Some((row, at)) = frame.caret {
            let x = origin.x - px(frame.offset.0) + cell.width * at as f32;
            let y = origin.y + cell.height * row as f32;
            let width = px(desktop::EDITOR_CARET_WIDTH);
            let caret = Bounds::new(point(x, y), size(width, cell.height));
            window.paint_quad(fill(caret, color(palette.accent)));
        }
    });
    paint_gutter(frame, bounds, origin.y, palette, window, cx);
    paint_scrollbar(frame, bounds, palette, window);
}

/// Fills runs of cells, each `(from, to)` counted from the first cell shown, at the row whose
/// first cell is at `(x, y)`.
fn shade(
    window: &mut Window,
    (x, y): (Pixels, Pixels),
    cell: CellSize,
    cells: &[(usize, usize)],
    role: u32,
) {
    for (from, to) in cells {
        let area = Bounds::new(
            point(x + cell.width * *from as f32, y),
            size(cell.width * (to - from) as f32, cell.height),
        );
        window.paint_quad(fill(area, color(role)));
    }
}

/// A run of text of `length` bytes in `color`, in the editor's face.
fn mono_run(length: usize, color: Hsla) -> TextRun {
    TextRun {
        len: length,
        font: font(MONO_FAMILY),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// One line of text, in runs of the colors of its syntax: the plain characters each on its cell,
/// a wide character or a letter with its accents from its cell.
fn draw_line(
    shown: &Shown,
    first: Pixels,
    y: Pixels,
    cell: CellSize,
    palette: &Palette,
    (window, cx): (&mut Window, &mut App),
) {
    for part in &shown.parts {
        let runs: Vec<TextRun> = part
            .runs
            .iter()
            .map(|(length, kind)| mono_run(*length, color(kind_color(palette, kind.as_ref()))))
            .collect();
        let layout = if part.spread {
            Layout::Grid(cell)
        } else {
            Layout::Natural(cell.height)
        };
        let at = point(first + cell.width * part.at as f32, y);
        paint_runs(
            &part.text,
            &runs,
            editor::FONT_POINT_SIZE,
            at,
            layout,
            window,
            cx,
        );
    }
}

/// The line numbers beside the lines, on the face so that text scrolled sideways passes under
/// them; the line of the caret has the well behind its number.
fn paint_gutter(
    frame: &Frame,
    bounds: Bounds<Pixels>,
    top: Pixels,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) {
    let cell = frame.cell;
    let gutter = Bounds::new(
        bounds.origin,
        size(px(editor::GUTTER_WIDTH), bounds.size.height),
    );
    window.paint_quad(fill(gutter, color(palette.face)));
    window.with_content_mask(Some(ContentMask { bounds: gutter }), |window| {
        for (index, row) in frame.rows.iter().enumerate() {
            let y = top + cell.height * index as f32;
            if frame.caret.is_some_and(|(caret, _)| caret == index) {
                let band = Bounds::new(
                    point(gutter.left(), y),
                    size(gutter.size.width, cell.height),
                );
                window.paint_quad(fill(band, color(palette.well)));
            }
            let label = row.number.to_string();
            let run = mono_run(label.len(), color(palette.muted));
            let shaped =
                window
                    .text_system()
                    .shape_line(label.into(), px(type_scale::SMALL), &[run], None);
            let x = gutter.right() - shaped.width - px(editor::TEXT_INSET);
            if let Err(error) =
                shaped.paint(point(x, y), cell.height, TextAlign::Left, None, window, cx)
            {
                eprintln!("soyeht: could not paint a line number: {error}");
            }
        }
    });
}

/// A thin thumb on the right edge that shows where the lines in view are in the whole text. It
/// only shows: the wheel and the caret scroll.
fn paint_scrollbar(frame: &Frame, bounds: Bounds<Pixels>, palette: &Palette, window: &mut Window) {
    let (scrolled, view) = frame.scroll;
    if frame.content <= view || view <= 0.0 {
        return;
    }
    let track = f32::from(bounds.size.height) - 2.0 * desktop::EDITOR_THUMB_INSET;
    let length = (track * view / frame.content)
        .max(desktop::EDITOR_THUMB_MINIMUM)
        .min(track);
    let travel = track - length;
    let share = scrolled / (frame.content - view);
    let thumb = Bounds::new(
        point(
            bounds.right() - px(desktop::EDITOR_THUMB_WIDTH + desktop::EDITOR_THUMB_INSET),
            bounds.top() + px(desktop::EDITOR_THUMB_INSET + travel * share.clamp(0.0, 1.0)),
        ),
        size(px(desktop::EDITOR_THUMB_WIDTH), px(length)),
    );
    window.paint_quad(
        fill(thumb, color(palette.well))
            .corner_radii(Corners::all(px(desktop::EDITOR_THUMB_WIDTH / 2.0))),
    );
}

/// How many characters fit in `width` points.
pub fn columns_in(width: Pixels, cell: CellSize) -> usize {
    ((width / cell.width).floor().max(1.0) as usize) + desktop::EDITOR_MARGIN_CELLS as usize
}
