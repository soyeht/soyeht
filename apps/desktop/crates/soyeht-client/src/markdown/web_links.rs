//! Web addresses typed in the middle of a reply, without Markdown around them.
//!
//! The Mac pane links them (`WebLinkRouting.links`); the cases both clients pass are in
//! `contracts/vectors/web-links.json`. An address starts at `http://` or `https://` (any
//! case) and runs to the next space, angle bracket or quote. The punctuation a sentence puts
//! after it is not part of it. An address with no host, or with a name and password in it, is
//! not linked. Code is never linked: a code span or block is shown as it was written.

use super::{Block, Run, Style};
use soyeht_protocol::{client_policy::CAPS_WEB_LINKS, limits::WEB_FIELD_BYTES};
use std::ops::Range;

/// What a sentence puts after an address and the address does not own.
const TRAILING: [char; 9] = ['.', ',', ';', ':', '!', '?', ')', ']', '}'];

/// The byte ranges of the web addresses in `text`, in order.
pub fn bare_addresses(text: &str) -> Vec<Range<usize>> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while found.len() < CAPS_WEB_LINKS {
        let Some(start) = next_scheme(&lower, from) else {
            break;
        };
        let end = text[start..]
            .find(is_boundary)
            .map_or(text.len(), |offset| start + offset);
        from = end;
        let trimmed = text[start..end].trim_end_matches(TRAILING).len();
        let address = &text[start..start + trimmed];
        if usable(address) {
            found.push(start..start + trimmed);
        }
    }
    found
}

/// The blocks with their bare addresses linked: in the runs of text that are neither code nor
/// already a link. Code blocks stay as they are.
pub fn link_blocks(blocks: Vec<Block>) -> Vec<Block> {
    map_runs(blocks, &link_runs)
}

/// The blocks with `change` applied to the runs of every paragraph, heading, quote, list item and
/// table cell. Code blocks and rules stay as they are.
pub(super) fn map_runs(blocks: Vec<Block>, change: &dyn Fn(Vec<Run>) -> Vec<Run>) -> Vec<Block> {
    blocks
        .into_iter()
        .map(|block| match block {
            Block::Paragraph(runs) => Block::Paragraph(change(runs)),
            Block::Heading { level, runs } => Block::Heading {
                level,
                runs: change(runs),
            },
            Block::Quote(blocks) => Block::Quote(map_runs(blocks, change)),
            Block::List { start, items } => Block::List {
                start,
                items: items
                    .into_iter()
                    .map(|item| map_runs(item, change))
                    .collect(),
            },
            Block::Table { header, rows } => Block::Table {
                header: header.into_iter().map(change).collect(),
                rows: rows
                    .into_iter()
                    .map(|row| row.into_iter().map(change).collect())
                    .collect(),
            },
            other @ (Block::Code { .. } | Block::Rule) => other,
        })
        .collect()
}

fn link_runs(runs: Vec<Run>) -> Vec<Run> {
    let mut linked: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.style.code || run.style.link.is_some() {
            linked.push(run);
            continue;
        }
        let mut rest = 0;
        for range in bare_addresses(&run.text) {
            push(&mut linked, &run, rest..range.start, None);
            let address = run.text[range.clone()].to_owned();
            let link = |style: &mut Style| style.link = Some(address.clone());
            push(&mut linked, &run, range.clone(), Some(&link));
            rest = range.end;
        }
        push(&mut linked, &run, rest..run.text.len(), None);
    }
    linked
}

/// Appends the part `range` of `run`, with `mark` applied to its style when there is one, merged
/// into the run before it when the two look the same.
pub(super) fn push(
    runs: &mut Vec<Run>,
    run: &Run,
    range: Range<usize>,
    mark: Option<&dyn Fn(&mut Style)>,
) {
    if range.is_empty() {
        return;
    }
    let mut style = run.style.clone();
    if let Some(mark) = mark {
        mark(&mut style);
    }
    let text = &run.text[range];
    match runs.last_mut() {
        Some(last) if last.style == style => last.text.push_str(text),
        _ => runs.push(Run {
            text: text.to_owned(),
            style,
        }),
    }
}

/// Where the next `http://` or `https://` of `lower` (already lowercased) starts at or after
/// `from`, when something follows it.
fn next_scheme(lower: &str, from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(offset) = lower.get(at..)?.find("http") {
        let start = at + offset;
        let after = &lower[start + 4..];
        let scheme = if after.starts_with("s://") {
            8
        } else if after.starts_with("://") {
            7
        } else {
            0
        };
        if scheme > 0 && lower[start + scheme..].starts_with(|c: char| !is_boundary(c)) {
            return Some(start);
        }
        at = start + 4;
    }
    None
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'')
}

/// An address with a host and no name or password, short enough to open.
fn usable(address: &str) -> bool {
    if address.len() > WEB_FIELD_BYTES {
        return false;
    }
    let Some((_, rest)) = address.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    let host = match authority.strip_prefix('[') {
        Some(inside) => inside.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    !host.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{Style, parse};

    fn addresses(text: &str) -> Vec<&str> {
        bare_addresses(text)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    #[test]
    fn a_long_address_is_not_linked() {
        let long = format!("https://a.test/{}", "x".repeat(WEB_FIELD_BYTES));
        assert!(addresses(&long).is_empty());
    }

    #[test]
    fn no_more_than_the_limit_are_linked() {
        let text = "https://a.test ".repeat(CAPS_WEB_LINKS + 5);
        assert_eq!(bare_addresses(&text).len(), CAPS_WEB_LINKS);
    }

    #[test]
    fn an_address_keeps_the_style_of_the_words_around_it() {
        let blocks = link_blocks(parse("**see https://a.test/x.** now", false));
        let Block::Paragraph(runs) = &blocks[0] else {
            panic!("a paragraph");
        };
        let texts: Vec<_> = runs.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(texts, ["see ", "https://a.test/x", ".", " now"]);
        assert!(runs[..3].iter().all(|run| run.style.bold));
        assert_eq!(runs[1].style.link.as_deref(), Some("https://a.test/x"));
        assert_eq!(runs[0].style.link, None);
        assert_eq!(runs[3].style, Style::default());
    }

    #[test]
    fn code_and_written_links_are_left_alone() {
        let source = "`https://a.test/c` [t](https://a.test/l) [https://a.test/m](https://a.test/n)\n\n\
                      ```\nhttps://a.test/block\n```";
        let blocks = parse(source, false);
        let linked = link_blocks(blocks.clone());
        assert_eq!(linked, blocks);
    }

    #[test]
    fn lists_quotes_and_tables_are_linked_too() {
        for (source, address) in [
            ("- https://a.test/l", "https://a.test/l"),
            ("> https://a.test/q", "https://a.test/q"),
            ("| h |\n|---|\n| https://a.test/t |", "https://a.test/t"),
        ] {
            let shown = format!("{:?}", link_blocks(parse(source, false)));
            assert!(
                shown.contains(&format!("link: Some({address:?})")),
                "{source}"
            );
        }
    }
}
