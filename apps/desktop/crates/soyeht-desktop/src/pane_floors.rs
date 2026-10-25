//! The smallest each kind of pane is drawn at: the Mac's measured floors
//! (`PaneLayoutFloorFormula`), ported with the same tokens (`pane_layout` of the token file). A
//! floor is what the pane's content needs: columns of the terminal, the editor's lines and its
//! gutter, the agent's composer, a writing pane's page.
//!
//! The Mac measures its fonts; this client takes the metrics of the floor report as the
//! `reference*` tokens, the same ones `contracts/vectors/pane-layout.json` measures its floors
//! with, and both clients pass that file. Floors are of the pane's card, without the halo of its
//! shadow, which the gaps between panes hold.

use crate::theme::{desktop, editor, pane_layout as layout};
use soyeht_client::layout::Sizing;
use soyeht_protocol::WorkspacePaneLayoutKind as Kind;

/// A width and a height in points.
type Size = (f64, f64);

/// What the formula needs to know of a font: the width of a character, the height of a line and
/// the size it is set in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FontMetric {
    pub character_width: f64,
    pub line_height: f64,
    pub point_size: f64,
}

/// The fonts a pane's content is set in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FontMetrics {
    pub terminal: FontMetric,
    pub editor: FontMetric,
    pub chrome: FontMetric,
    pub agent: FontMetric,
    pub prose: FontMetric,
    pub screenplay: FontMetric,
}

impl FontMetrics {
    /// The metrics of the floor report, which the bundled fonts have.
    pub(crate) fn reference() -> Self {
        let metric = |character_width: f32, line_height: f32, point_size: f32| FontMetric {
            character_width: f64::from(character_width),
            line_height: f64::from(line_height),
            point_size: f64::from(point_size),
        };
        Self {
            terminal: metric(
                desktop::REFERENCE_TERMINAL_CHARACTER_WIDTH,
                desktop::REFERENCE_TERMINAL_LINE_HEIGHT,
                desktop::REFERENCE_TERMINAL_POINT_SIZE,
            ),
            editor: metric(
                desktop::REFERENCE_EDITOR_CHARACTER_WIDTH,
                desktop::REFERENCE_EDITOR_LINE_HEIGHT,
                desktop::REFERENCE_EDITOR_POINT_SIZE,
            ),
            chrome: metric(
                desktop::REFERENCE_CHROME_CHARACTER_WIDTH,
                desktop::REFERENCE_CHROME_LINE_HEIGHT,
                desktop::REFERENCE_CHROME_POINT_SIZE,
            ),
            agent: metric(
                desktop::REFERENCE_AGENT_CHARACTER_WIDTH,
                desktop::REFERENCE_AGENT_LINE_HEIGHT,
                desktop::REFERENCE_AGENT_POINT_SIZE,
            ),
            prose: metric(
                desktop::REFERENCE_PROSE_CHARACTER_WIDTH,
                desktop::REFERENCE_PROSE_LINE_HEIGHT,
                desktop::REFERENCE_PROSE_POINT_SIZE,
            ),
            screenplay: metric(
                desktop::REFERENCE_SCREENPLAY_CHARACTER_WIDTH,
                desktop::REFERENCE_SCREENPLAY_LINE_HEIGHT,
                desktop::REFERENCE_SCREENPLAY_POINT_SIZE,
            ),
        }
    }
}

fn token(value: f32) -> f64 {
    f64::from(value)
}

fn card_inset() -> f64 {
    token(layout::PANE_CARD_INSET)
}

fn header() -> f64 {
    token(layout::HEADER_HEIGHT)
}

fn stack_spacing() -> f64 {
    token(layout::PANE_STACK_SPACING)
}

/// The width the editor's lines need: their columns, the gutter and the text inset on both sides.
fn editor_code_width(fonts: &FontMetrics) -> f64 {
    token(layout::EDITOR_COLUMNS) * fonts.editor.character_width
        + token(editor::GUTTER_WIDTH)
        + 2.0 * token(editor::TEXT_INSET)
}

/// The width an editor's lines keep beside the Markdown preview.
pub(crate) fn editor_code_floor(fonts: &FontMetrics) -> f64 {
    editor_code_width(fonts).ceil()
}

/// The editor's floor; `project_panel` adds the panel beside the lines.
fn editor_floor(fonts: &FontMetrics, project_panel: bool) -> Size {
    let panel = if project_panel {
        token(editor::PROJECT_PANEL_WIDTH)
    } else {
        0.0
    };
    (
        (editor_code_width(fonts) + 2.0 * card_inset() + panel).ceil(),
        2.0 * card_inset()
            + token(editor::HEADER_HEIGHT)
            + stack_spacing()
            + token(editor::TAB_HEIGHT)
            + token(layout::EDITOR_MINIMUM_ROWS) * token(editor::LINE_HEIGHT)
            + token(editor::VIEWPORT_VERTICAL_INSET)
            + token(layout::EDITOR_REVIEW_HEIGHT)
            + token(editor::STATUS_HEIGHT),
    )
}

/// The floor of every kind of pane, with the editor's project panel open.
pub(crate) fn surface_floors(fonts: &FontMetrics) -> Vec<(Kind, Size)> {
    let terminal = (
        (token(layout::TERMINAL_COLUMNS) * fonts.terminal.character_width + 2.0 * card_inset())
            .ceil(),
        (2.0 * card_inset()
            + header()
            + stack_spacing()
            + (fonts.chrome.line_height + 2.0 * token(layout::TERMINAL_STATE_VERTICAL_INSET))
            + token(layout::TERMINAL_ROWS) * fonts.terminal.line_height)
            .ceil(),
    );
    let agent = (
        (token(layout::COMPOSER_CHARACTERS) * fonts.agent.character_width
            + token(layout::COMPOSER_CONTROL_CHROME_WIDTH))
        .ceil(),
        2.0 * card_inset()
            + header()
            + stack_spacing()
            + token(layout::AGENT_BODY_TOP_INSET)
            + token(layout::AGENT_TASK_HEIGHT)
            + token(layout::AGENT_BODY_BOTTOM_INSET)
            + token(layout::AGENT_CONTROLS_FIXTURE_HEIGHT)
            + token(layout::AGENT_DIVIDER_HEIGHT)
            + token(layout::AGENT_TRANSCRIPT_FIXTURE_HEIGHT)
            + token(layout::AGENT_COMPOSER_FIXTURE_HEIGHT),
    );
    let web = (
        token(layout::WEB_VIEWPORT_WIDTH) + 2.0 * card_inset(),
        2.0 * card_inset()
            + header()
            + stack_spacing()
            + token(layout::WEB_NAVIGATION_HEIGHT)
            + stack_spacing()
            + token(layout::WEB_VIEWPORT_HEIGHT)
            + stack_spacing()
            + token(layout::WEB_STATUS_HEIGHT),
    );
    let text = (
        (token(layout::COMPACT_TEXT_CHARACTERS) * fonts.prose.character_width
            + 2.0 * token(layout::TEXT_PAGE_INSET)
            + 2.0 * card_inset())
        .ceil(),
        token(layout::WRITING_HEADER_HEIGHT)
            + token(layout::EDITOR_REVIEW_HEIGHT)
            + 2.0 * stack_spacing()
            + 2.0 * token(layout::TEXT_PAGE_INSET)
            + token(layout::TEXT_COMPACT_LINES) * token(layout::TEXT_LINE_HEIGHT)
            + card_inset(),
    );
    let screenplay_page = token(layout::SCREENPLAY_COLUMNS) * fonts.screenplay.character_width
        + 2.0 * token(layout::SCREENPLAY_PAGE_HORIZONTAL_INSET);
    let zoom = token(layout::MINIMUM_SCREENPLAY_POINT_SIZE) / fonts.screenplay.point_size;
    let screenplay_width = (screenplay_page * zoom).ceil();
    let screenplay = (
        screenplay_width,
        token(layout::WRITING_HEADER_HEIGHT)
            + (screenplay_width * token(layout::SCREENPLAY_PAGE_HEIGHT_INCHES)
                / token(layout::SCREENPLAY_PAGE_WIDTH_INCHES))
            .ceil(),
    );
    let presentation = (
        token(layout::PRESENTATION_STAGE_WIDTH) + 2.0 * card_inset(),
        token(layout::WRITING_HEADER_HEIGHT)
            + token(layout::PRESENTATION_NUMERIC_RAIL_HEIGHT)
            + token(layout::PRESENTATION_STAGE_HEIGHT)
            + token(layout::PRESENTATION_FOOTER_HEIGHT)
            + 2.0 * card_inset(),
    );
    vec![
        (Kind::Terminal, terminal),
        (Kind::Editor, editor_floor(fonts, true)),
        (Kind::Agent, agent),
        (Kind::Web, web),
        (Kind::Text, text),
        (Kind::Screenplay, screenplay),
        (Kind::Presentation, presentation),
        (Kind::UnknownWriting, screenplay),
    ]
}

/// The floors of the kinds that fold optional chrome away: the editor without its project panel.
pub(crate) fn compact_floors(fonts: &FontMetrics) -> Vec<(Kind, Size)> {
    vec![(Kind::Editor, editor_floor(fonts, false))]
}

/// Whether an editor whose text area (the editor without its project panel) is
/// `text_area_width` wide shows the Markdown preview beside its lines: only when the area holds
/// the lines at their floor, the divider and the preview at its narrowest. Narrower, the lines
/// keep the room.
pub(crate) fn editor_shows_preview(text_area_width: f64, fonts: &FontMetrics) -> bool {
    text_area_width
        >= (editor_code_width(fonts)
            + token(layout::EDITOR_PREVIEW_DIVIDER_WIDTH)
            + token(editor::PREVIEW_MIN_WIDTH))
        .ceil()
}

/// [`editor_shows_preview`] for the editor's measured width, with the reference fonts.
pub(crate) fn preview_fits(text_area_width: f32) -> bool {
    editor_shows_preview(f64::from(text_area_width), &FontMetrics::reference())
}

/// What an arrangement is measured with: each kind of pane needs its floor, and the room between
/// two panes is the gap. With the project panel hidden the editor needs its compact floor; with
/// it shown, the full one.
pub(crate) fn sizing(project_panel: bool) -> Sizing {
    let fonts = FontMetrics::reference();
    let mut kinds = surface_floors(&fonts);
    if !project_panel {
        for (kind, size) in compact_floors(&fonts) {
            if let Some(entry) = kinds.iter_mut().find(|(known, _)| *known == kind) {
                entry.1 = size;
            }
        }
    }
    let gap = token(layout::PANE_GAP);
    Sizing {
        floor: kinds
            .iter()
            .find(|(kind, _)| *kind == Kind::Terminal)
            .map_or((0.0, 0.0), |(_, size)| *size),
        kinds,
        horizontal_gap: gap,
        vertical_gap: gap,
        companion_strip: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn vector() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../contracts/vectors/pane-layout.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn metric(held: &Value) -> FontMetric {
        let number = |key: &str| held[key].as_f64().unwrap();
        FontMetric {
            character_width: number("characterWidth"),
            line_height: number("lineHeight"),
            point_size: number("pointSize"),
        }
    }

    fn vector_fonts(held: &Value) -> FontMetrics {
        let fonts = &held["fonts"];
        FontMetrics {
            terminal: metric(&fonts["terminal"]),
            editor: metric(&fonts["editor"]),
            chrome: metric(&fonts["chrome"]),
            agent: metric(&fonts["agent"]),
            prose: metric(&fonts["prose"]),
            screenplay: metric(&fonts["screenplay"]),
        }
    }

    /// The kind a name of the vector stands for: the vector spells `unknownWriting` as the Mac
    /// does, the protocol as `unknown_writing`.
    fn named(kind: &str) -> Kind {
        let wire = if kind == "unknownWriting" {
            "unknown_writing"
        } else {
            kind
        };
        serde_json::from_value(serde_json::json!(wire)).unwrap()
    }

    fn pair(value: &Value) -> Size {
        (value[0].as_f64().unwrap(), value[1].as_f64().unwrap())
    }

    /// The floors hold exactly the kinds `held` names, at the sizes it gives.
    fn assert_floors(floors: &[(Kind, Size)], held: &Value) {
        let held = held.as_object().unwrap();
        assert_eq!(floors.len(), held.len());
        for (name, size) in held {
            let found = floors.iter().find(|(kind, _)| *kind == named(name));
            assert_eq!(found.map(|(_, size)| *size), Some(pair(size)), "{name}");
        }
    }

    #[test]
    fn every_kind_of_pane_has_the_floor_the_vector_says() {
        let all = vector();
        let held = &all["kindFloors"];
        assert_floors(&surface_floors(&vector_fonts(held)), &held["surfaces"]);
    }

    #[test]
    fn a_kind_that_folds_chrome_away_has_the_compact_floor_the_vector_says() {
        let all = vector();
        let held = &all["kindFloors"];
        assert_floors(
            &compact_floors(&vector_fonts(held)),
            &held["compactSurfaces"],
        );
    }

    #[test]
    fn the_editor_shows_the_preview_when_its_text_area_holds_the_lines_and_the_preview() {
        let all = vector();
        let held = &all["kindFloors"];
        let fonts = vector_fonts(held);
        for case in held["editorPreview"].as_array().unwrap() {
            assert_eq!(
                editor_shows_preview(case["textAreaWidth"].as_f64().unwrap(), &fonts),
                case["shows"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    /// The tokens this client measures with are the metrics the vector measures its floors with,
    /// so passing the vector is passing for the floors it draws.
    #[test]
    fn the_reference_metrics_are_the_ones_the_vector_measures_with() {
        let all = vector();
        let held = vector_fonts(&all["kindFloors"]);
        let reference = FontMetrics::reference();
        let pairs = [
            (held.terminal, reference.terminal),
            (held.editor, reference.editor),
            (held.chrome, reference.chrome),
            (held.agent, reference.agent),
            (held.prose, reference.prose),
            (held.screenplay, reference.screenplay),
        ];
        for (expected, got) in pairs {
            assert!((expected.character_width - got.character_width).abs() < 1e-4);
            assert_eq!(expected.line_height, got.line_height);
            assert_eq!(expected.point_size, got.point_size);
        }
    }

    #[test]
    fn hiding_the_project_panel_lowers_only_the_editor_floor() {
        let (shown, hidden) = (sizing(true), sizing(false));
        let of = |sizing: &Sizing, kind: Kind| {
            sizing
                .kinds
                .iter()
                .find(|(known, _)| *known == kind)
                .map(|(_, size)| *size)
        };
        for (kind, size) in &shown.kinds {
            if *kind == Kind::Editor {
                let hidden = of(&hidden, Kind::Editor).unwrap();
                assert_eq!(size.0 - hidden.0, f64::from(editor::PROJECT_PANEL_WIDTH));
                assert_eq!(size.1, hidden.1);
            } else {
                assert_eq!(of(&hidden, kind.clone()), Some(*size));
            }
        }
    }

    #[test]
    fn every_kind_of_pane_has_a_floor() {
        let kinds = sizing(true).kinds;
        for name in [
            "terminal",
            "agent",
            "editor",
            "web",
            "text",
            "presentation",
            "screenplay",
            "unknownWriting",
        ] {
            assert!(kinds.iter().any(|(kind, _)| *kind == named(name)), "{name}");
        }
    }

    #[test]
    fn a_divider_dragged_far_stops_at_the_floor_of_the_kind_beside_it() {
        let tree: soyeht_protocol::WorkspacePaneLayoutNode =
            serde_json::from_value(serde_json::json!({
                "axis": "horizontal",
                "children": [
                    {"weight": 1.0, "node": {"paneId": "e", "kind": "editor"}},
                    {"weight": 1.0, "node": {"paneId": "t", "kind": "terminal"}}
                ]
            }))
            .unwrap();
        let sizing = sizing(true);
        let viewport = (1200.0, 800.0);
        // The room beyond both floors and the gap is shared out by weight: a pane is at its
        // floor when its weight is nothing beside the other's.
        for (distance, floored) in [(1000.0, 1), (-1000.0, 0)] {
            let dragged =
                soyeht_client::layout::resize(&tree, &[], 0, distance, viewport, &sizing).unwrap();
            let soyeht_protocol::WorkspacePaneLayoutNode::V1(split) = dragged else {
                panic!("a split stays a split");
            };
            let weights: Vec<f64> = split.children.iter().map(|branch| branch.weight).collect();
            let total: f64 = weights.iter().sum();
            assert!(weights[floored] / total < 1e-8, "{weights:?}");
        }
    }

    #[test]
    fn the_state_measures_a_workspace_by_whether_its_project_panel_is_shown() {
        let mut state = soyeht_client::State::default().with_sizing(crate::views::sizing);
        let editor_width = |state: &soyeht_client::State| {
            state
                .sizing_of("w")
                .kinds
                .iter()
                .find(|(kind, _)| *kind == Kind::Editor)
                .map(|(_, size)| size.0)
                .unwrap()
        };
        let shown = editor_width(&state);
        state.toggle_project_panel("w");
        assert_eq!(
            shown - editor_width(&state),
            f64::from(editor::PROJECT_PANEL_WIDTH)
        );
        state.toggle_project_panel("w");
        assert_eq!(editor_width(&state), shown);
    }
}
