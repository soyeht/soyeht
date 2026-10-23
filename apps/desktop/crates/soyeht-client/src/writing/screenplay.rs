//! How a screenplay is written, as the Mac and the desktop client both do it: which element Tab
//! and Return move on to, the edit they make, and how a line sits on the page (aligned, set in,
//! bold or in the secondary ink) with the characters of it that are markers and stay hidden. The
//! engine says which element a line is (`writing.structure`); the rest is decided here. The
//! cases are `contracts/vectors/screenplay.json`, which the Mac passes too
//! (`ScreenplayVectorTests`).

use super::document::change_between;
use soyeht_protocol::{ScreenplayElement, ScreenplayElementKind as Kind};
use std::ops::Range;

/// The element a line moves on to when Return (or Tab) ends it.
pub fn next_kind(kind: &Kind, tab: bool) -> Kind {
    match (kind, tab) {
        (Kind::SceneHeading, _) => Kind::Action,
        (Kind::Character | Kind::Parenthetical, _) => Kind::Dialogue,
        (Kind::Dialogue, true) => Kind::Parenthetical,
        (Kind::Dialogue, false) => Kind::Character,
        (Kind::Action, true) => Kind::Character,
        (Kind::Transition | Kind::Shot, _) => Kind::SceneHeading,
        _ => Kind::Action,
    }
}

/// The marks that force an element, which are not part of its text.
const FORCING: [char; 4] = ['.', '@', '>', '<'];

fn unforced(text: &str) -> &str {
    text.trim_matches(&FORCING[..]).trim()
}

/// `source` set as an element: the line as it is written in the file.
pub fn formatted_line(source: &str, kind: &Kind) -> String {
    let text = source.trim();
    match kind {
        Kind::SceneHeading if text.is_empty() => ".INT. ".to_owned(),
        Kind::SceneHeading => format!(".{}", unforced(text).to_uppercase()),
        Kind::Character => format!("@{}", unforced(text).to_uppercase()),
        Kind::Parenthetical => format!("({})", text.trim_matches(&['(', ')'][..])),
        Kind::Transition => format!(">{}", unforced(text).to_uppercase()),
        Kind::Shot => {
            let upper = unforced(text).to_uppercase();
            if upper.ends_with(':') {
                upper
            } else {
                format!("{upper}:")
            }
        }
        Kind::Action | Kind::Dialogue => unforced(text).to_owned(),
    }
}

/// An edit of the text: `range` (bytes) is replaced by `replacement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advance {
    pub range: Range<usize>,
    pub replacement: String,
}

/// What Return (or Tab) does at byte `cursor` of `source`, in a line of element `kind`: the
/// line stays, a blank line follows, and the next element starts on the line after that with the
/// mark that makes it that element. Tab on a line with nothing on it only seeds the next element
/// there. `None` when `cursor` is not a place of the text.
pub fn advance_edit(source: &str, cursor: usize, kind: &Kind, tab: bool) -> Option<Advance> {
    if !source.is_char_boundary(cursor) {
        return None;
    }
    let start = source[..cursor].rfind('\n').map_or(0, |found| found + 1);
    let end = source[cursor..]
        .find('\n')
        .map_or(source.len(), |found| cursor + found);
    let content = &source[start..end];
    let seed = formatted_line("", &next_kind(kind, tab));
    let replacement = if tab && content.trim().is_empty() {
        seed
    } else {
        format!("{content}\n\n{seed}")
    };
    Some(Advance {
        range: start..end,
        replacement,
    })
}

/// Where a line is set between the margins of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    #[default]
    Natural,
    Center,
    Left,
    Right,
}

/// How a line sits on the page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineStyle {
    pub alignment: Alignment,
    /// Set in from both margins, as a speech is.
    pub inset: bool,
    pub bold: bool,
    /// Drawn in the secondary ink instead of the primary one.
    pub secondary: bool,
    /// The markers of the line, as byte ranges of it, in order and apart: they stay in the text
    /// and are not shown.
    pub hidden: Vec<Range<usize>>,
}

/// The style of a line of element `kind` written as `line`.
pub fn line_style(kind: &Kind, line: &str) -> LineStyle {
    let hidden = markers(kind, line);
    let mut style = LineStyle {
        alignment: Alignment::Natural,
        inset: false,
        bold: false,
        secondary: false,
        hidden,
    };
    match kind {
        Kind::SceneHeading | Kind::Shot => style.bold = true,
        Kind::Action => {}
        Kind::Character => {
            style.alignment = Alignment::Center;
            style.bold = true;
        }
        Kind::Parenthetical => {
            style.alignment = Alignment::Center;
            style.secondary = true;
        }
        Kind::Dialogue => style.inset = true,
        Kind::Transition => {
            style.alignment = if opens(line, &style.hidden) {
                Alignment::Left
            } else {
                Alignment::Right
            };
            style.secondary = true;
        }
    }
    style
}

/// The characters of a line that mark its element and are not shown.
fn markers(kind: &Kind, line: &str) -> Vec<Range<usize>> {
    let mut hidden = Vec::new();
    if line.len() >= ACTION.len()
        && line.is_char_boundary(ACTION.len())
        && line[..ACTION.len()].eq_ignore_ascii_case(ACTION)
        && *kind == Kind::Action
    {
        let example = line[ACTION.len()..]
            .bytes()
            .take_while(|byte| *byte == b' ')
            .count();
        hidden.push(0..ACTION.len() + example);
    }
    let forced = matches!(
        kind,
        Kind::SceneHeading | Kind::Character | Kind::Transition
    ) && line.starts_with(['.', '@', '>']);
    if forced {
        hidden.push(0..1);
    }
    let last = line.len().saturating_sub(1)..line.len();
    if (*kind == Kind::Transition && line.ends_with('>'))
        || (*kind == Kind::Character && line.ends_with(':'))
    {
        hidden.push(last);
    }
    hidden.sort_by_key(|range| range.start);
    let mut apart: Vec<Range<usize>> = Vec::new();
    for range in hidden {
        match apart.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => apart.push(range),
        }
    }
    apart
}

const ACTION: &str = "[action]";

/// Whether a transition is the one a screenplay opens with, which sits on the left.
fn opens(line: &str, hidden: &[Range<usize>]) -> bool {
    let mut shown = String::new();
    let mut from = 0;
    for range in hidden {
        shown.push_str(&line[from..range.start]);
        from = range.end;
    }
    shown.push_str(&line[from..]);
    shown.trim().to_uppercase() == "FADE IN:"
}

/// The element each line of a screenplay is, as far as is known. The engine tells it for the
/// text it was asked about; while the person types, the lines move with the edits (a line
/// inserted has no element until the engine is asked again), so that the page does not lose its
/// look between one answer and the next.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineKinds {
    /// The text the kinds are of.
    text: String,
    kinds: Vec<Option<Kind>>,
}

impl LineKinds {
    /// The kinds the engine gave for `text`.
    pub fn of(text: &str, elements: &[ScreenplayElement]) -> Self {
        let mut kinds = vec![None; text.split('\n').count()];
        for element in elements {
            let (first, last) = (element.start_line as usize, element.end_line as usize);
            for slot in kinds.iter_mut().take(last + 1).skip(first) {
                *slot = Some(element.kind.clone());
            }
        }
        Self {
            text: text.to_owned(),
            kinds,
        }
    }

    /// The text changed to `after`: the lines that were not touched keep their kind, wherever
    /// they moved to.
    pub fn follow(&mut self, after: &str) {
        let Some(change) = change_between(&self.text, after, 0) else {
            return;
        };
        let (first, last) = (
            change.range.start.line as usize,
            change.range.end.line as usize,
        );
        let breaks = change.text.matches('\n').count();
        let kind = |line: usize| self.kinds.get(line).cloned().flatten();
        // What is left of the line the change began at stands before it, and what is left of the
        // line it ended at stands after it: when nothing of the first is left, the last one's
        // element is the element of the line that remains.
        let head_empty = change.range.start.column == 0;
        let mut next: Vec<Option<Kind>> = (0..first).map(kind).collect();
        if breaks == 0 {
            next.push(if head_empty && change.text.is_empty() {
                kind(last)
            } else {
                kind(first)
            });
        } else if head_empty {
            next.extend(std::iter::repeat_n(None, breaks));
            next.push(change.text.ends_with('\n').then(|| kind(last)).flatten());
        } else {
            next.push(kind(first));
            next.extend(std::iter::repeat_n(None, breaks));
        }
        next.extend((last + 1..self.kinds.len()).map(kind));
        self.kinds = next;
        self.text = after.to_owned();
    }

    /// The line is the element `kind`, as the person has just made it.
    pub fn assume(&mut self, line: usize, kind: Kind) {
        if line >= self.kinds.len() {
            self.kinds.resize(line + 1, None);
        }
        self.kinds[line] = Some(kind);
    }

    /// The element of a line, when it is known.
    pub fn kind_at(&self, line: usize) -> Option<&Kind> {
        self.kinds.get(line).and_then(Option::as_ref)
    }

    /// The style of each line of `text`, which the kinds follow.
    pub fn styles(&self, text: &str) -> Vec<LineStyle> {
        text.split('\n')
            .enumerate()
            .map(|(line, written)| {
                self.kind_at(line)
                    .map_or_else(LineStyle::default, |kind| line_style(kind, written))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        editor::text::{byte_at, utf16_len},
        testing::{cases, text},
    };
    use serde_json::Value;

    fn kind(value: &Value) -> Kind {
        serde_json::from_value(value.clone()).unwrap()
    }

    fn number(value: &Value) -> u32 {
        u32::try_from(value.as_u64().unwrap()).unwrap()
    }

    #[test]
    fn tab_and_return_move_to_the_element_the_vector_says() {
        let all = cases("screenplay", "advance");
        assert!(!all.is_empty());
        for case in all {
            assert_eq!(
                next_kind(&kind(&case["kind"]), case["tab"].as_bool().unwrap()),
                kind(&case["next"]),
                "{case}"
            );
        }
    }

    #[test]
    fn an_element_is_set_as_the_line_the_vector_says() {
        for case in cases("screenplay", "formatting") {
            assert_eq!(
                formatted_line(text(&case["source"]), &kind(&case["kind"])),
                text(&case["line"]),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn tab_and_return_make_the_edit_the_vector_says() {
        for case in cases("screenplay", "advanceEdit") {
            let source = text(&case["source"]);
            let cursor = byte_at(source, number(&case["cursor"]));
            let edit = advance_edit(
                source,
                cursor,
                &kind(&case["kind"]),
                case["tab"].as_bool().unwrap(),
            )
            .unwrap();
            let range = &case["range"];
            assert_eq!(
                (
                    utf16_len(&source[..edit.range.start]),
                    utf16_len(&source[edit.range.clone()])
                ),
                (number(&range["location"]), number(&range["length"])),
                "{}",
                case["name"]
            );
            assert_eq!(
                edit.replacement,
                text(&case["replacement"]),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn a_line_sits_on_the_page_as_the_vector_says() {
        for case in cases("screenplay", "lineStyle") {
            let line = text(&case["line"]);
            let style = line_style(&kind(&case["kind"]), line);
            let alignment = match text(&case["alignment"]) {
                "center" => Alignment::Center,
                "left" => Alignment::Left,
                "right" => Alignment::Right,
                _ => Alignment::Natural,
            };
            let name = &case["name"];
            assert_eq!(style.alignment, alignment, "{name}");
            assert_eq!(style.inset, case["inset"].as_bool().unwrap(), "{name}");
            assert_eq!(style.bold, case["bold"].as_bool().unwrap(), "{name}");
            assert_eq!(
                style.secondary,
                case["secondary"].as_bool().unwrap(),
                "{name}"
            );
            let hidden: Vec<(u32, u32)> = style
                .hidden
                .iter()
                .map(|range| {
                    (
                        utf16_len(&line[..range.start]),
                        utf16_len(&line[range.clone()]),
                    )
                })
                .collect();
            let expected: Vec<(u32, u32)> = case["hidden"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| (number(&pair[0]), number(&pair[1])))
                .collect();
            assert_eq!(hidden, expected, "{name}");
        }
    }

    #[test]
    fn a_marker_alone_is_hidden_once() {
        let style = line_style(&Kind::Transition, ">");
        assert_eq!(style.hidden, vec![0..1]);
    }

    #[test]
    fn the_lines_that_were_not_touched_keep_their_element() {
        let mut kinds = LineKinds::of("INT. ROOM\n\n@ANA\nHello", &[]);
        kinds.kinds = vec![
            Some(Kind::SceneHeading),
            None,
            Some(Kind::Character),
            Some(Kind::Dialogue),
        ];
        // A line typed at the end of the heading keeps the heading's element.
        kinds.follow("INT. ROOM - DAY\n\n@ANA\nHello");
        assert_eq!(kinds.kind_at(0), Some(&Kind::SceneHeading));
        // A line break inside the dialogue makes a line that is not known yet.
        kinds.follow("INT. ROOM - DAY\n\n@ANA\nHel\nlo");
        assert_eq!(kinds.kind_at(3), Some(&Kind::Dialogue));
        assert_eq!(kinds.kind_at(4), None);
        // Lines added above move the ones below down.
        kinds.follow("Early.\n\nINT. ROOM - DAY\n\n@ANA\nHel\nlo");
        assert_eq!((kinds.kind_at(0), kinds.kind_at(1)), (None, None));
        assert_eq!(kinds.kind_at(2), Some(&Kind::SceneHeading));
        assert_eq!(kinds.kind_at(5), Some(&Kind::Dialogue));
        // A line taken out moves them up, and the heading is still the heading.
        kinds.follow("Early.\nINT. ROOM - DAY\n\n@ANA\nHel\nlo");
        assert_eq!(kinds.kind_at(1), Some(&Kind::SceneHeading));
        assert_eq!(kinds.kind_at(4), Some(&Kind::Dialogue));
        // A line break at the start of a line pushes that line down with its element.
        kinds.follow("Early.\n\nINT. ROOM - DAY\n\n@ANA\nHel\nlo");
        assert_eq!(kinds.kind_at(1), None);
        assert_eq!(kinds.kind_at(2), Some(&Kind::SceneHeading));
    }

    #[test]
    fn a_line_made_a_known_element_has_its_style() {
        let mut kinds = LineKinds::default();
        kinds.follow("a\n@");
        kinds.assume(1, Kind::Character);
        let styles = kinds.styles("a\n@");
        assert_eq!(styles[0], LineStyle::default());
        assert!(styles[1].bold && styles[1].alignment == Alignment::Center);
        assert_eq!(styles[1].hidden, vec![0..1]);
    }

    #[test]
    fn the_engine_gives_the_kind_of_each_line_of_its_elements() {
        let element = |line: u32, kind: &str| -> ScreenplayElement {
            serde_json::from_value(serde_json::json!({
                "elementId": format!("e{line}"), "kind": kind,
                "startLine": line, "endLine": line, "text": ""
            }))
            .unwrap()
        };
        let kinds = LineKinds::of(
            "INT. A\n\nANA\nHi",
            &[
                element(0, "scene_heading"),
                element(2, "character"),
                element(3, "dialogue"),
            ],
        );
        assert_eq!(kinds.kind_at(0), Some(&Kind::SceneHeading));
        assert_eq!(kinds.kind_at(1), None);
        assert_eq!(kinds.kind_at(3), Some(&Kind::Dialogue));
    }
}
