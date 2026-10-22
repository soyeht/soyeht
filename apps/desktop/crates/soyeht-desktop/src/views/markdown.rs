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
use soyeht_client::markdown::{self, Block, Run, Style};
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

    /// The blocks of the Markdown preview of the file an editor pane shows.
    pub(crate) fn preview_blocks(&mut self, pane_id: &str, source: &str) -> Arc<[Block]> {
        self.cache.blocks(&preview_owner(pane_id), source, false)
    }

    /// Drops what is kept for the preview of a pane that is closed.
    pub(crate) fn forget_preview(&mut self, pane_id: &str) {
        self.cache.forget(&preview_owner(pane_id));
    }
}

/// What names the preview of an editor pane in the cache, among the messages' ids.
fn preview_owner(pane_id: &str) -> String {
    format!("preview-{pane_id}")
}

/// What a block needs to draw: the palette, the text color of the surrounding block, and a name
/// for the links' click targets.
struct Draw<'a> {
    palette: &'static Palette,
    ink: u32,
    owner: &'a str,
    opens: Opens<'a>,
    paragraphs: usize,
    codes: usize,
}

/// What a click on a link in the drawn text does.
#[derive(Clone, Copy)]
pub(super) enum Opens<'a> {
    /// Web addresses open in the browser, and the references of a pane's text open in its
    /// workspace's editor when `Files` finds them there.
    Links(Option<&'a Files>),
    /// Links take the accent and an underline and open nothing, and a file reference is plain
    /// text: a file's content is not trusted to open anything (the editor's preview, as the
    /// Mac's, ADR 0053 and 0087).
    Nothing,
}

/// The blocks of one message, in `ink`. `owner` names the message, for its links.
pub(super) fn reply(
    blocks: &[Block],
    owner: &str,
    palette: &'static Palette,
    ink: u32,
    opens: Opens,
) -> AnyElement {
    let mut draw = Draw {
        palette,
        ink,
        owner,
        opens,
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

/// Whether a run is drawn as a link, and what a click on it opens. A written link opens its
/// address; a file reference opens its file when it is in the workspace; with `Opens::Nothing`
/// a link is only drawn and a reference is plain text.
fn link_of(style: &Style, opens: Opens) -> (bool, Option<Link>) {
    let file = match opens {
        Opens::Links(files) => style
            .file
            .as_ref()
            .and_then(|reference| files?.resolve(reference)),
        Opens::Nothing => None,
    };
    let linked = style.link.is_some() || file.is_some();
    let target = match opens {
        Opens::Links(_) => style
            .link
            .clone()
            .map(Link::Web)
            .or_else(|| file.map(Link::File)),
        Opens::Nothing => None,
    };
    (linked, target)
}

/// Runs of text as one block of text: bold, italic, struck, code and linked looks as highlights
/// of one string, so that it wraps as a whole. A link opens in the system browser, unless the
/// text is inert.
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
        let (linked, target) = link_of(style, draw.opens);
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
        if let Some(target) = target {
            links.push((range, target));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_open::Files;
    use soyeht_client::file_links::Reference;

    fn linked_to(address: &str) -> Style {
        Style {
            link: Some(address.to_owned()),
            ..Style::default()
        }
    }

    #[test]
    fn a_link_in_a_reply_opens_its_address() {
        let (looks_linked, target) = link_of(&linked_to("https://example.com"), Opens::Links(None));
        assert!(looks_linked);
        assert!(matches!(target, Some(Link::Web(address)) if address == "https://example.com"));
    }

    #[test]
    fn a_link_in_a_preview_looks_like_one_and_opens_nothing() {
        let (looks_linked, target) = link_of(&linked_to("https://example.com"), Opens::Nothing);
        assert!(looks_linked);
        assert!(target.is_none());
    }

    #[test]
    fn a_file_reference_is_plain_text_in_a_preview_and_without_a_workspace() {
        let style = Style {
            file: Some(Reference {
                path: "src/main.rs".to_owned(),
                line: 11,
                column: None,
            }),
            ..Style::default()
        };
        let files: Option<&Files> = None;
        for opens in [Opens::Nothing, Opens::Links(files)] {
            let (looks_linked, target) = link_of(&style, opens);
            assert!(!looks_linked && target.is_none());
        }
    }

    #[test]
    fn a_closed_panes_preview_is_parsed_again_and_the_others_are_kept() {
        let mut replies = Replies::default();
        let closed = replies.preview_blocks("e1", "# one");
        let kept = replies.preview_blocks("e2", "# two");
        replies.forget_preview("e1");
        assert!(!Arc::ptr_eq(
            &closed,
            &replies.preview_blocks("e1", "# one")
        ));
        assert!(Arc::ptr_eq(&kept, &replies.preview_blocks("e2", "# two")));
    }
}
