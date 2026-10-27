//! How much of a reply one frame lays out.
//!
//! A frame lays out every element it is given, and a reply is one row of the transcript with no
//! bound on its blocks: a message may hold 256 KiB of Markdown, which is thousands of list items,
//! and the newest messages are laid out again by every repaint. So a reply is drawn a page at a
//! time: the first [`PAGE_WEIGHT`] of its weight, then a "Show more" row that adds the next page.
//! The weight stands for the layout cost whatever the shape: a paragraph, a list item or a table
//! cell is one unit, and a long run of text adds one for every [`BYTES_PER_UNIT`] bytes.
//! The copy button of a reply still copies the whole message.

use super::{Block, Run};
use std::sync::Arc;

/// The label of the row that adds the next page of a reply.
pub const SHOW_MORE: &str = "Show more";
/// What stands over the end of a reply that is still arriving, in place of its beginning.
pub const EARLIER_HIDDEN: &str = "The beginning shows when the reply is done";
/// What a page of a reply weighs: about a hundred list items.
pub const PAGE_WEIGHT: usize = 120;
/// The bytes of text that weigh as much as one block: a paragraph of a few lines.
const BYTES_PER_UNIT: usize = 240;

/// The layout cost of text of this many bytes: a row of a conversation that is not a reply.
pub fn text_weight(bytes: usize) -> usize {
    1 + bytes / BYTES_PER_UNIT
}

fn runs_bytes(runs: &[Run]) -> usize {
    runs.iter().map(|run| run.text.len()).sum()
}

/// The layout cost of a row of a table: its cells.
fn row_weight(row: &[Vec<Run>]) -> usize {
    row.iter().map(|cell| text_weight(runs_bytes(cell))).sum()
}

/// The layout cost of a block.
fn weight(block: &Block) -> usize {
    match block {
        Block::Paragraph(runs) | Block::Heading { runs, .. } => text_weight(runs_bytes(runs)),
        Block::Code { text, .. } => text_weight(text.len()),
        Block::Quote(blocks) => weight_of(blocks),
        Block::List { items, .. } => items.iter().map(|item| weight_of(item).max(1)).sum(),
        Block::Table { header, rows } => {
            row_weight(header) + rows.iter().map(|row| row_weight(row)).sum::<usize>()
        }
        Block::Rule => 1,
    }
}

/// The layout cost of a reply.
pub fn weight_of(blocks: &[Block]) -> usize {
    blocks.iter().map(weight).sum()
}

/// A reply cut to the pages that are shown.
#[derive(Debug, Clone)]
pub struct Shown {
    pub blocks: Arc<[Block]>,
    /// Whether part of the reply is held back: the reply is drawn with a "Show more" row, or, when
    /// `tail` says it is the end that is shown, with a line saying the beginning is held back.
    pub more: bool,
    /// The shown part is the end of a reply that is still arriving ([`shown_tail`]).
    pub tail: bool,
}

impl Shown {
    /// What laying the shown part out costs, with the "Show more" row when there is one.
    pub fn weight(&self) -> usize {
        weight_of(&self.blocks) + usize::from(self.more)
    }
}

/// The first `pages` pages of `blocks` (at least one). A reply that fits is returned as it is.
pub fn shown(blocks: &Arc<[Block]>, pages: usize) -> Shown {
    let limit = pages.max(1) * PAGE_WEIGHT;
    if weight_of(blocks) <= limit {
        return Shown {
            blocks: Arc::clone(blocks),
            more: false,
            tail: false,
        };
    }
    Shown {
        blocks: cut(blocks, limit).into(),
        more: true,
        tail: false,
    }
}

/// The last `pages` pages of a reply that is still arriving, so that what it adds stays in view
/// while the reply grows: a page's worth is laid out whatever the length, and the first pages
/// are shown once the reply is done ([`shown`]).
pub fn shown_tail(blocks: &Arc<[Block]>, pages: usize) -> Shown {
    let limit = pages.max(1) * PAGE_WEIGHT;
    let total = weight_of(blocks);
    if total <= limit {
        return shown(blocks, pages);
    }
    Shown {
        blocks: skip(blocks, total - limit).into(),
        more: true,
        tail: true,
    }
}

/// The blocks after the first `drop` of weight; the block that holds the end of the drop is cut
/// where it can be.
fn skip(blocks: &[Block], mut drop: usize) -> Vec<Block> {
    let mut kept = Vec::new();
    for block in blocks {
        let weight = weight(block);
        if drop >= weight {
            drop -= weight;
        } else if drop == 0 {
            kept.push(block.clone());
        } else {
            kept.extend(skip_block(block, drop));
            drop = 0;
        }
    }
    kept
}

fn skip_block(block: &Block, drop: usize) -> Option<Block> {
    match block {
        Block::Paragraph(runs) => Some(Block::Paragraph(skip_runs(runs, drop))),
        Block::Heading { level, runs } => Some(Block::Heading {
            level: *level,
            runs: skip_runs(runs, drop),
        }),
        Block::Code { language, text } => {
            let start = text.ceil_char_boundary(drop * BYTES_PER_UNIT);
            let rest = &text[start..];
            // Start at the beginning of a line when there is one in reach.
            let rest = rest.find('\n').map_or(rest, |line| &rest[line + 1..]);
            Some(Block::Code {
                language: language.clone(),
                text: rest.to_owned(),
            })
        }
        Block::Quote(blocks) => {
            let kept = skip(blocks, drop);
            (!kept.is_empty()).then_some(Block::Quote(kept))
        }
        Block::List { start, items } => {
            let mut drop = drop;
            let mut kept = Vec::new();
            for item in items {
                let weight = weight_of(item).max(1);
                if drop >= weight {
                    drop -= weight;
                } else if drop == 0 {
                    kept.push(item.clone());
                } else {
                    let rest = skip(item, drop);
                    if !rest.is_empty() {
                        kept.push(rest);
                    }
                    drop = 0;
                }
            }
            // The numbering goes on from where the shown items start.
            let skipped = items.len() - kept.len();
            (!kept.is_empty()).then(|| Block::List {
                start: start.map(|first| first + skipped as u64),
                items: kept,
            })
        }
        Block::Table { header, rows } => {
            let mut drop = drop.saturating_sub(row_weight(header));
            let mut kept = Vec::new();
            for row in rows {
                if drop == 0 {
                    kept.push(row.clone());
                } else {
                    drop = drop.saturating_sub(row_weight(row));
                }
            }
            Some(Block::Table {
                header: header.clone(),
                rows: kept,
            })
        }
        Block::Rule => None,
    }
}

/// The runs after the first `drop` units of text, cut at a character.
fn skip_runs(runs: &[Run], drop: usize) -> Vec<Run> {
    let mut left = drop * BYTES_PER_UNIT;
    let mut kept = Vec::new();
    for run in runs {
        let start = run.text.ceil_char_boundary(left.min(run.text.len()));
        left -= start;
        if start < run.text.len() {
            kept.push(Run {
                text: run.text[start..].to_owned(),
                style: run.style.clone(),
            });
        }
    }
    kept
}

/// The blocks, up to `limit` in weight; the block that crosses the limit is cut where it can be
/// (a list between items, a table between rows, text and code at a character).
fn cut(blocks: &[Block], limit: usize) -> Vec<Block> {
    let mut kept = Vec::new();
    let mut used = 0;
    for block in blocks {
        let weight = weight(block);
        if used + weight <= limit {
            kept.push(block.clone());
            used += weight;
        } else {
            kept.extend(cut_block(block, limit - used));
            break;
        }
    }
    kept
}

fn cut_block(block: &Block, room: usize) -> Option<Block> {
    if room == 0 {
        return None;
    }
    match block {
        Block::Paragraph(runs) => Some(Block::Paragraph(cut_runs(runs, room))),
        Block::Heading { level, runs } => Some(Block::Heading {
            level: *level,
            runs: cut_runs(runs, room),
        }),
        Block::Code { language, text } => Some(Block::Code {
            language: language.clone(),
            text: cut_code(text, bytes_in(room)),
        }),
        Block::Quote(blocks) => {
            let kept = cut(blocks, room);
            (!kept.is_empty()).then_some(Block::Quote(kept))
        }
        Block::List { start, items } => {
            let mut kept = Vec::new();
            let mut used = 0;
            for item in items {
                let weight = weight_of(item).max(1);
                if used + weight <= room {
                    kept.push(item.clone());
                    used += weight;
                } else {
                    let rest = cut(item, room - used);
                    if !rest.is_empty() {
                        kept.push(rest);
                    }
                    break;
                }
            }
            (!kept.is_empty()).then_some(Block::List {
                start: *start,
                items: kept,
            })
        }
        Block::Table { header, rows } => {
            let mut used = row_weight(header);
            if used > room {
                return None;
            }
            let mut kept = Vec::new();
            for row in rows {
                used += row_weight(row);
                if used > room {
                    break;
                }
                kept.push(row.clone());
            }
            Some(Block::Table {
                header: header.clone(),
                rows: kept,
            })
        }
        Block::Rule => None,
    }
}

/// The most bytes of text that weigh `room` units.
fn bytes_in(room: usize) -> usize {
    room * BYTES_PER_UNIT - 1
}

/// The runs up to `room` units of text, cut at a character.
fn cut_runs(runs: &[Run], room: usize) -> Vec<Run> {
    let mut left = bytes_in(room);
    let mut kept = Vec::new();
    for run in runs {
        if left == 0 {
            break;
        }
        let end = char_boundary(&run.text, left);
        left -= end;
        kept.push(Run {
            text: run.text[..end].to_owned(),
            style: run.style.clone(),
        });
    }
    kept
}

/// The code up to `bytes`, cut at the end of a line when it has one in reach.
fn cut_code(text: &str, bytes: usize) -> String {
    let end = char_boundary(text, bytes);
    let head = &text[..end];
    if end == text.len() {
        return head.to_owned();
    }
    head.rfind('\n')
        .map_or(head, |line| &head[..line])
        .to_owned()
}

/// The largest index up to `at` that is a character boundary of `text`.
fn char_boundary(text: &str, at: usize) -> usize {
    let mut end = at.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests;
