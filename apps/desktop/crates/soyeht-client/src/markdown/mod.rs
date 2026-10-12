//! Agent replies as blocks: the Markdown subset every Soyeht client draws.
//!
//! The rules live in `contracts/vectors/markdown.json`, which the Swift client passes too
//! (`MarkdownTests`). This module maps a CommonMark parser's events onto the vector's shape:
//! paragraphs, headings, fenced and indented code, block quotes, bullet and ordered lists,
//! tables and rules, with runs of text styled bold, italic, struck, code or linked.
//! A link is kept only for http, https and mailto addresses, and one whose text is its own
//! address is plain text (the pane linkifies bare addresses itself). Raw HTML stays literal.
//! A line break is a `\n` in the text. Empty code blocks and empty list items are dropped.
//!
//! Parsing is a pure function of the text, so a reply that is still arriving is re-parsed each
//! time it grows; [`streaming::close`] first closes what the reply has left open, so a bold
//! word or a code span shows its style as it is typed instead of showing raw markers.

mod streaming;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::{collections::HashMap, sync::Arc};

/// How a run of text is drawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<String>,
}

/// Text in one style. Neighbouring runs of the same style are merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Run>),
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Quote(Vec<Block>),
    List {
        /// The first number of an ordered list; `None` for a bullet list.
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Table {
        header: Vec<Vec<Run>>,
        rows: Vec<Vec<Vec<Run>>>,
    },
    Rule,
}

/// Parses a reply. `streaming` is true while the reply is still arriving.
pub fn parse(text: &str, streaming: bool) -> Vec<Block> {
    let closed;
    let text = if streaming {
        closed = streaming::close(text);
        closed.as_str()
    } else {
        text
    };
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut builder = Builder::new();
    for event in Parser::new_ext(text, options) {
        builder.event(event);
    }
    builder.finish()
}

/// A block that is still collecting its children.
enum Open {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    Item(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
}

#[derive(Default)]
struct Table {
    header: Vec<Vec<Run>>,
    rows: Vec<Vec<Vec<Run>>>,
    row: Vec<Vec<Run>>,
}

struct Builder {
    open: Vec<Open>,
    runs: Vec<Run>,
    /// Depths of the emphasis tags that are open (a tag closes the innermost of its kind).
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<Option<String>>,
    /// Where the runs of the open link start, to take the link off when it names itself.
    link_starts: Vec<usize>,
    code: Option<(Option<String>, String)>,
    html: Option<String>,
    table: Option<Table>,
}

impl Builder {
    fn new() -> Self {
        Self {
            open: vec![Open::Root(Vec::new())],
            runs: Vec::new(),
            bold: 0,
            italic: 0,
            strike: 0,
            links: Vec::new(),
            link_starts: Vec::new(),
            code: None,
            html: None,
            table: None,
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(text) => self.push_run(&text, true),
            Event::InlineHtml(text) => self.text(&text),
            Event::Html(text) => match self.html.as_mut() {
                Some(html) => html.push_str(&text),
                None => self.text(&text),
            },
            Event::SoftBreak | Event::HardBreak => self.push_run("\n", false),
            Event::TaskListMarker(done) => self.push_run(if done { "[x] " } else { "[ ] " }, false),
            Event::Rule => {
                self.flush();
                self.push_block(Block::Rule);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => {
                self.links.push(allowed(&dest_url));
                self.link_starts.push(self.runs.len());
            }
            Tag::Image { dest_url, .. } => {
                self.links.push(allowed(&dest_url));
                self.link_starts.push(self.runs.len());
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.code = Some((language(&kind), String::new()));
            }
            Tag::HtmlBlock => {
                self.flush();
                self.html = Some(String::new());
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.open.push(Open::Quote(Vec::new()));
            }
            Tag::List(start) => {
                self.flush();
                self.open.push(Open::List {
                    start,
                    items: Vec::new(),
                });
            }
            Tag::Item => {
                self.flush();
                self.open.push(Open::Item(Vec::new()));
            }
            Tag::Table(_) => {
                self.flush();
                self.table = Some(Table::default());
            }
            Tag::Paragraph | Tag::Heading { .. } => self.flush(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link | TagEnd::Image => self.end_link(),
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(level) => {
                let runs = std::mem::take(&mut self.runs);
                self.push_block(Block::Heading {
                    level: heading_level(level),
                    runs,
                });
            }
            TagEnd::CodeBlock => self.end_code(),
            TagEnd::HtmlBlock => {
                if let Some(html) = self.html.take() {
                    let text = html.trim_end_matches('\n');
                    if !text.is_empty() {
                        self.push_block(Block::Paragraph(vec![Run {
                            text: text.to_owned(),
                            style: Style::default(),
                        }]));
                    }
                }
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                if let Some(Open::Quote(blocks)) = self.open.pop() {
                    self.push_block(Block::Quote(blocks));
                }
            }
            TagEnd::Item => {
                self.flush();
                if let Some(Open::Item(blocks)) = self.open.pop()
                    && !blocks.is_empty()
                    && let Some(Open::List { items, .. }) = self.open.last_mut()
                {
                    items.push(blocks);
                }
            }
            TagEnd::List(_) => {
                if let Some(Open::List { start, items }) = self.open.pop()
                    && !items.is_empty()
                {
                    self.push_block(Block::List { start, items });
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.runs);
                if let Some(table) = self.table.as_mut() {
                    table.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.header = std::mem::take(&mut table.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(mut table) = self.table.take() {
                    let width = table.header.len();
                    for row in &mut table.rows {
                        row.resize(width, Vec::new());
                    }
                    self.push_block(Block::Table {
                        header: table.header,
                        rows: table.rows,
                    });
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        match (self.code.as_mut(), self.html.as_mut()) {
            (Some((_, code)), _) => code.push_str(text),
            (None, Some(html)) => html.push_str(text),
            (None, None) => self.push_run(text, false),
        }
    }

    fn push_run(&mut self, text: &str, code: bool) {
        if text.is_empty() {
            return;
        }
        let style = Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code,
            link: self.links.iter().rev().find_map(Clone::clone),
        };
        match self.runs.last_mut() {
            Some(last) if last.style == style => last.text.push_str(text),
            _ => self.runs.push(Run {
                text: text.to_owned(),
                style,
            }),
        }
    }

    /// Takes the link off the runs of a link that names itself, then merges what became equal.
    fn end_link(&mut self) {
        let start = self.link_starts.pop().unwrap_or(0).min(self.runs.len());
        let Some(Some(address)) = self.links.pop() else {
            return;
        };
        let text: String = self.runs[start..]
            .iter()
            .map(|run| run.text.as_str())
            .collect();
        if text == address {
            for run in &mut self.runs[start..] {
                run.style.link = None;
            }
            self.merge_from(start.saturating_sub(1));
        }
    }

    fn merge_from(&mut self, start: usize) {
        let mut merged: Vec<Run> = Vec::with_capacity(self.runs.len());
        for (index, run) in std::mem::take(&mut self.runs).into_iter().enumerate() {
            match merged.last_mut() {
                Some(last) if index > start && last.style == run.style => {
                    last.text.push_str(&run.text);
                }
                _ => merged.push(run),
            }
        }
        self.runs = merged;
    }

    fn end_paragraph(&mut self) {
        let runs = std::mem::take(&mut self.runs);
        if !runs.is_empty() {
            self.push_block(Block::Paragraph(runs));
        }
    }

    fn end_code(&mut self) {
        if let Some((language, text)) = self.code.take() {
            let text = text.strip_suffix('\n').unwrap_or(&text);
            if !text.is_empty() {
                self.push_block(Block::Code {
                    language,
                    text: text.to_owned(),
                });
            }
        }
    }

    /// Text that came without a paragraph (a tight list item) becomes one.
    fn flush(&mut self) {
        if self.table.is_none() {
            self.end_paragraph();
        }
    }

    fn push_block(&mut self, block: Block) {
        if let Some(Open::Root(blocks) | Open::Quote(blocks) | Open::Item(blocks)) =
            self.open.last_mut()
        {
            blocks.push(block);
        }
    }

    fn finish(mut self) -> Vec<Block> {
        self.flush();
        match self.open.into_iter().next() {
            Some(Open::Root(blocks)) => blocks,
            _ => Vec::new(),
        }
    }
}

fn language(kind: &CodeBlockKind) -> Option<String> {
    match kind {
        CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .map(str::to_owned)
            .filter(|name| !name.is_empty()),
        CodeBlockKind::Indented => None,
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// The address of a link a pane may open; any other scheme (or none) is drawn as text.
fn allowed(address: &str) -> Option<String> {
    let lower = address.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
        .then(|| address.to_owned())
}

/// The parse of every reply a transcript shows, kept by message, so that a frame that changes
/// nothing about a reply does not parse it again.
#[derive(Default)]
pub struct Cache {
    entries: HashMap<String, Entry>,
}

struct Entry {
    text: String,
    streaming: bool,
    blocks: Arc<[Block]>,
}

/// More replies than a transcript shows at once; past it, the cache starts over.
const CACHE_LIMIT: usize = 1024;

impl Cache {
    /// The blocks of the message `owner` with this text, parsed only when they changed.
    pub fn blocks(&mut self, owner: &str, text: &str, streaming: bool) -> Arc<[Block]> {
        if let Some(entry) = self.entries.get(owner)
            && entry.streaming == streaming
            && entry.text == text
        {
            return Arc::clone(&entry.blocks);
        }
        if self.entries.len() >= CACHE_LIMIT {
            self.entries.clear();
        }
        let blocks: Arc<[Block]> = parse(text, streaming).into();
        self.entries.insert(
            owner.to_owned(),
            Entry {
                text: text.to_owned(),
                streaming,
                blocks: Arc::clone(&blocks),
            },
        );
        blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_that_did_not_change_is_not_parsed_again() {
        let mut cache = Cache::default();
        let first = cache.blocks("m1", "a **b**", false);
        let again = cache.blocks("m1", "a **b**", false);
        assert!(Arc::ptr_eq(&first, &again));
        let grown = cache.blocks("m1", "a **b** c", false);
        assert!(!Arc::ptr_eq(&first, &grown));
        let finished = cache.blocks("m1", "a **b** c", true);
        assert!(
            !Arc::ptr_eq(&grown, &finished),
            "streaming changes the parse"
        );
    }

    #[test]
    fn a_cache_that_grew_too_large_starts_over() {
        let mut cache = Cache::default();
        for index in 0..=CACHE_LIMIT {
            cache.blocks(&index.to_string(), "text", false);
        }
        assert!(cache.entries.len() <= CACHE_LIMIT);
    }

    #[test]
    fn text_with_no_markdown_is_one_plain_paragraph() {
        assert_eq!(
            parse("just words", false),
            [Block::Paragraph(vec![Run {
                text: "just words".into(),
                style: Style::default()
            }])]
        );
    }
}
