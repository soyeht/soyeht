//! An agent's words with their Markdown drawn, in the approved flat interior: paragraphs,
//! headings, lists, quotes, tables and rules in the palette, and code in a mono well like the
//! one a command's output sits in. The Mac draws the same blocks (`MarkdownViews.swift`);
//! both read them from `contracts/vectors/markdown.json`'s rules.

use super::{
    copy_button::copyable,
    selectable::{Link, Selectable},
    ui::{code_well, column, row, rule},
};
use crate::{
    file_open::{Files, Probes},
    theme::{Palette, fonts, pane_interior},
};
use gpui::{
    AnyElement, Div, FontStyle, FontWeight, HighlightStyle, IntoElement, ParentElement,
    SharedString, StrikethroughStyle, Styled, UnderlineStyle, div, px, rgb,
};
use soyeht_client::markdown::{self, Block, Run};
use std::{cell::RefCell, ops::Range, rc::Rc, sync::Arc};

/// The Markdown of the replies on screen, parsed once per change, and the files that the pane
/// being drawn names (`src/main.rs:12` opens in the editor when the file is in its workspace).
#[derive(Default)]
pub(crate) struct Replies {
    cache: markdown::Cache,
    pub(super) files: Option<Files>,
    /// What the window knows of whether the files that replies name are there.
    pub(crate) probes: Rc<RefCell<Probes>>,
}

impl Replies {
    pub(super) fn blocks(&mut self, owner: &str, text: &str, streaming: bool) -> Arc<[Block]> {
        self.cache.blocks(owner, text, streaming)
    }
}

/// What a block needs to draw: the palette, the text color of the surrounding block, and a name
/// for the links' click targets.
struct Draw<'a> {
    palette: &'static Palette,
    ink: u32,
    owner: &'a str,
    files: Option<&'a Files>,
    paragraphs: usize,
    codes: usize,
}

/// The blocks of one message, in `ink`. `owner` names the message, for its links.
pub(super) fn reply(
    blocks: &[Block],
    owner: &str,
    palette: &'static Palette,
    ink: u32,
    files: Option<&Files>,
) -> AnyElement {
    let mut draw = Draw {
        palette,
        ink,
        owner,
        files,
        paragraphs: 0,
        codes: 0,
    };
    stack(blocks, &mut draw).into_any_element()
}

fn stack(blocks: &[Block], draw: &mut Draw) -> Div {
    column()
        .w_full()
        .min_w_0()
        .gap(px(pane_interior::COMPACT_SPACING))
        .children(
            blocks
                .iter()
                .map(|block| self::block(block, draw))
                .collect::<Vec<_>>(),
        )
}

fn block(block: &Block, draw: &mut Draw) -> AnyElement {
    let palette = draw.palette;
    match block {
        Block::Paragraph(runs) => text(runs, false, draw),
        Block::Heading { runs, .. } => text(runs, true, draw),
        Block::Code { text, .. } => {
            let key = format!("{}-code-{}", draw.owner, draw.codes);
            draw.codes += 1;
            let selectable = Selectable::plain(key.clone(), text.clone(), palette);
            copyable(
                code_well(selectable, palette).text_color(rgb(draw.ink)),
                &key,
                text.clone(),
                pane_interior::CARD_PADDING,
                palette,
            )
            .into_any_element()
        }
        Block::Quote(blocks) => {
            let outer = std::mem::replace(&mut draw.ink, palette.secondary);
            let content = stack(blocks, draw);
            draw.ink = outer;
            row()
                .items_stretch()
                .gap(px(pane_interior::ROW_SPACING))
                .child(
                    div()
                        .flex_none()
                        .w(px(pane_interior::TIGHT_SPACING))
                        .bg(rgb(palette.well)),
                )
                .child(content.flex_1())
                .into_any_element()
        }
        Block::List { start, items } => column()
            .gap(px(pane_interior::TIGHT_SPACING))
            .children(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let marker = start.map_or_else(
                            || "\u{2022}".to_owned(),
                            |first| format!("{}.", first + index as u64),
                        );
                        row()
                            .items_start()
                            .gap(px(pane_interior::ROW_SPACING))
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(px(pane_interior::CARD_PADDING))
                                    .flex()
                                    .justify_end()
                                    .text_color(rgb(palette.secondary))
                                    .child(SharedString::from(marker)),
                            )
                            .child(stack(item, draw).flex_1())
                            .into_any_element()
                    })
                    .collect::<Vec<_>>(),
            )
            .into_any_element(),
        Block::Table { header, rows } => table(header, rows, draw),
        Block::Rule => rule(palette.well, pane_interior::SEPARATOR_WIDTH),
    }
}

/// A table: columns of equal width, the header in bold over a thin line.
fn table(header: &[Vec<Run>], rows: &[Vec<Vec<Run>>], draw: &mut Draw) -> AnyElement {
    let well = draw.palette.well;
    let mut line = |cells: &[Vec<Run>], bold: bool| {
        row()
            .items_start()
            .gap(px(pane_interior::ROW_SPACING))
            .children(
                cells
                    .iter()
                    .map(|cell| div().flex_1().min_w_0().child(text(cell, bold, draw)))
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
    };
    let mut lines = vec![
        line(header, true),
        rule(well, pane_interior::SEPARATOR_WIDTH),
    ];
    lines.extend(rows.iter().map(|cells| line(cells, false)));
    column()
        .gap(px(pane_interior::TIGHT_SPACING))
        .children(lines)
        .into_any_element()
}

/// Runs of text as one block of text: bold, italic, struck, code and linked looks as highlights
/// of one string, so that it wraps as a whole. A link opens in the system browser.
fn text(runs: &[Run], bold: bool, draw: &mut Draw) -> AnyElement {
    let palette = draw.palette;
    let mut content = String::new();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let mut mono: Vec<(Range<usize>, SharedString)> = Vec::new();
    let mut links: Vec<(Range<usize>, Link)> = Vec::new();
    for run in runs {
        let start = content.len();
        content.push_str(&run.text);
        let range = start..content.len();
        let style = &run.style;
        // A file reference is a link only when its file is in the workspace.
        let file = style
            .file
            .as_ref()
            .and_then(|reference| draw.files?.resolve(reference));
        let linked = style.link.is_some() || file.is_some();
        let highlight = HighlightStyle {
            color: linked.then(|| rgb(palette.accent).into()),
            font_weight: (bold || style.bold).then_some(FontWeight::BOLD),
            font_style: style.italic.then_some(FontStyle::Italic),
            background_color: style.code.then(|| rgb(palette.well).into()),
            underline: linked.then(|| UnderlineStyle {
                thickness: px(pane_interior::SEPARATOR_WIDTH),
                color: None,
                wavy: false,
            }),
            strikethrough: style.strike.then(|| StrikethroughStyle {
                thickness: px(pane_interior::SEPARATOR_WIDTH),
                color: None,
            }),
            ..HighlightStyle::default()
        };
        if highlight != HighlightStyle::default() {
            highlights.push((range.clone(), highlight));
        }
        if style.code {
            mono.push((range.clone(), SharedString::from(fonts::MONO)));
        }
        if let Some(link) = &style.link {
            links.push((range, Link::Web(link.clone())));
        } else if let Some(file) = file {
            links.push((range, Link::File(file)));
        }
    }
    let key = format!("{}-text-{}", draw.owner, draw.paragraphs);
    draw.paragraphs += 1;
    let element = Selectable {
        key: key.into(),
        content: content.into(),
        highlights,
        mono,
        links,
        palette,
    };
    div()
        .w_full()
        .text_color(rgb(draw.ink))
        .child(element)
        .into_any_element()
}
