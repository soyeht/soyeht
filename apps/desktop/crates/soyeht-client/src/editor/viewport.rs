//! When the visible lines need another page of the file (`EditorViewportPaging` on the Mac; the
//! cases are `contracts/vectors/editor.json`, which both clients pass).
//!
//! The editor holds one page: at most `limits::EDITOR_VIEWPORT_LINES` lines. Reading ahead keeps
//! the view inside it, a page that leads the view by a quarter, so that scrolling a long file
//! never waits for the engine.

use soyeht_protocol::{
    client_policy::{EDITOR_PAGE_LEAD_DIVISOR, EDITOR_PREFETCH_LINES},
    limits::EDITOR_VIEWPORT_LINES,
};

/// The lines of a page: the protocol's limit for one read.
pub const PAGE_LINES: u32 = EDITOR_VIEWPORT_LINES as u32;

/// How far a page leads the view that asked for it.
const LEAD: u32 = PAGE_LINES / EDITOR_PAGE_LEAD_DIVISOR;

/// What is loaded and what is in view, in document lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub visible_from: u32,
    pub visible_lines: u32,
    pub loaded_from: u32,
    pub loaded_to: u32,
    /// How many lines were asked for when the loaded page was read: the engine's first page of a
    /// file is a small one by choice, and a page it cuts short at its byte budget is as long as
    /// it can be.
    pub asked_lines: u32,
    pub total_lines: u32,
}

impl Window {
    /// The first line of the page to read, or `None` while the loaded page holds the view with
    /// room to spare, or when no other page would hold more of it.
    ///
    /// A page the engine cut short (it stops a page at its byte budget) is as long as a page can
    /// be there: it is settled when it holds the view, and one that does not is followed by the
    /// page that starts at the view, after which nothing more is asked.
    pub fn requested_start(self) -> Option<u32> {
        let loaded = self.loaded_to.saturating_sub(self.loaded_from);
        if self.total_lines <= loaded {
            return None;
        }
        let visible_to = self
            .visible_from
            .saturating_add(self.visible_lines.max(1))
            .min(self.total_lines);
        let cut = loaded < self.asked_lines;
        let margin = EDITOR_PREFETCH_LINES.min(loaded / 4);
        let protected_from = self.loaded_from.saturating_add(margin);
        let protected_to = self.loaded_to.saturating_sub(margin);
        let inside = self.visible_from >= protected_from && visible_to <= protected_to;
        if (loaded >= PAGE_LINES || cut) && inside {
            return None;
        }
        let last_start = self.total_lines.saturating_sub(PAGE_LINES);
        let start = self.visible_from.saturating_sub(LEAD).min(last_start);
        if cut {
            return self.next_after_a_cut_page(visible_to, start);
        }
        if start != self.loaded_from {
            return Some(start);
        }
        // The page that leads the view is the one loaded. A full page is the most there is, and a
        // short one that was asked for as short is read in full, unless it already ends the file.
        (loaded < PAGE_LINES && self.loaded_to < self.total_lines).then_some(start)
    }

    /// What follows a page that was cut short, `start` being the page that would lead the view.
    fn next_after_a_cut_page(self, visible_to: u32, start: u32) -> Option<u32> {
        let from = self.loaded_from;
        if self.visible_from >= from && visible_to <= self.loaded_to {
            return None;
        }
        let begins_inside = self.visible_from >= from && self.visible_from < self.loaded_to;
        if !begins_inside && start != from {
            return Some(start);
        }
        Some(self.visible_from).filter(|at| *at != from)
    }
}

/// The first line to read so that `line` is in view: the loaded page when it already holds the
/// line, otherwise a page with the line a quarter down. `asked_lines` is how many lines were asked
/// for when the loaded page was read: a page that came back shorter was cut short by the engine,
/// the next may be as short, and the line then leads by a quarter of that page, so that the page
/// read holds it. A page that has to be read again for it starts at the line itself.
pub fn landing_start(line: u32, loaded_from: u32, loaded_to: u32, asked_lines: u32) -> u32 {
    if line >= loaded_from && line < loaded_to {
        return loaded_from;
    }
    let loaded = loaded_to.saturating_sub(loaded_from);
    let lead = if loaded < asked_lines {
        LEAD.min(loaded / 4)
    } else {
        LEAD
    };
    line.saturating_sub(lead)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cases, vector};

    fn number(case: &serde_json::Value, key: &str) -> u32 {
        u32::try_from(case[key].as_u64().unwrap()).unwrap()
    }

    fn numbers(case: &serde_json::Value, key: &str) -> Vec<u64> {
        let list = case[key].as_array().unwrap();
        list.iter().map(|item| item.as_u64().unwrap()).collect()
    }

    #[test]
    fn the_page_is_the_protocols_limit() {
        assert_eq!(u64::from(PAGE_LINES), vector("editor")["pageLines"]);
    }

    #[test]
    fn the_visible_lines_ask_for_another_page_as_the_vector_says() {
        for case in cases("editor", "requestedStart") {
            let window = Window {
                visible_from: number(&case, "visibleFrom"),
                visible_lines: number(&case, "visibleLines"),
                loaded_from: number(&case, "loadedFrom"),
                loaded_to: number(&case, "loadedTo"),
                asked_lines: case["askedLines"].as_u64().map_or_else(
                    || number(&case, "loadedTo") - number(&case, "loadedFrom"),
                    |asked| u32::try_from(asked).unwrap(),
                ),
                total_lines: number(&case, "totalLines"),
            };
            let wanted = case["start"]
                .as_u64()
                .map(|start| u32::try_from(start).unwrap());
            assert_eq!(window.requested_start(), wanted, "{}", case["name"]);
        }
    }

    #[test]
    fn reads_settle_after_a_page_the_engine_cuts_short_as_the_vector_says() {
        for case in cases("editor", "settling") {
            let (total, cut) = (number(&case, "totalLines"), number(&case, "cutLines"));
            let (from, lines) = (number(&case, "visibleFrom"), number(&case, "visibleLines"));
            // The page `file.open` returns is a short one by choice.
            let (mut loaded_from, mut loaded_to, mut asked) = (0, total.min(200), 200);
            let mut reads = Vec::new();
            let ask = |loaded_from, loaded_to, asked, from| {
                Window {
                    visible_from: from,
                    visible_lines: lines,
                    loaded_from,
                    loaded_to,
                    asked_lines: asked,
                    total_lines: total,
                }
                .requested_start()
            };
            while let Some(start) = ask(loaded_from, loaded_to, asked, from) {
                reads.push(u64::from(start));
                assert!(reads.len() <= 8, "{}: reads do not settle", case["name"]);
                (loaded_from, loaded_to, asked) = (
                    start,
                    start + PAGE_LINES.min(cut).min(total - start),
                    PAGE_LINES,
                );
            }
            assert_eq!(reads, numbers(&case, "reads"), "{}", case["name"]);
            assert_eq!(
                [u64::from(loaded_from), u64::from(loaded_to)],
                [
                    case["page"][0].as_u64().unwrap(),
                    case["page"][1].as_u64().unwrap()
                ],
                "{}",
                case["name"]
            );
            // Scrolling a line asks for nothing more.
            assert_eq!(
                ask(loaded_from, loaded_to, asked, from + 1),
                None,
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn the_caret_reaches_a_line_past_a_page_the_engine_cut_short_as_the_vector_says() {
        for case in cases("editor", "navigation") {
            let (total, target) = (number(&case, "totalLines"), number(&case, "targetLine"));
            let cuts = numbers(&case, "cuts");
            let (mut from, mut to) = (number(&case, "loadedFrom"), number(&case, "loadedTo"));
            let mut reads = Vec::new();
            while !(from <= target && target < to) {
                assert!(reads.len() <= 4, "{}: never reaches the line", case["name"]);
                // The first read lands on the line; one that misses it is read again from it.
                let start = if reads.is_empty() {
                    landing_start(target, from, to, PAGE_LINES)
                } else {
                    target
                };
                let cut = u32::try_from(cuts[reads.len().min(cuts.len() - 1)]).unwrap();
                reads.push(u64::from(start));
                (from, to) = (start, start + PAGE_LINES.min(cut).min(total - start));
            }
            assert_eq!(reads, numbers(&case, "reads"), "{}", case["name"]);
        }
    }

    #[test]
    fn a_line_outside_the_page_lands_as_the_vector_says() {
        for case in cases("editor", "landingStart") {
            let start = landing_start(
                number(&case, "line"),
                number(&case, "loadedFrom"),
                number(&case, "loadedTo"),
                case["askedLines"]
                    .as_u64()
                    .map_or(0, |asked| u32::try_from(asked).unwrap()),
            );
            assert_eq!(u64::from(start), case["start"], "{}", case["name"]);
        }
    }
}
