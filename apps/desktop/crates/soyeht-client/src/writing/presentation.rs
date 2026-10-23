//! What the Mac and the desktop client both decide for a presentation, a Markdown document the
//! engine splits into slides: the words of a slide in the rail, which slide a line of the source
//! belongs to, how the pane is laid out (thumbnails or numbers beside a 16:9 stage), and what a
//! slide show does with a key, a click, and its position. The cases are
//! `contracts/vectors/presentation.json`, which the Mac passes too (`WritingPresentationVectorTests`
//! and `WritingLayoutPolicyTests`).

use soyeht_protocol::WritingSlide;

/// The title a slide shows: its heading, or the words that say it has none.
pub fn slide_title(heading: &str) -> &str {
    if heading.is_empty() {
        "Untitled slide"
    } else {
        heading
    }
}

/// What a slide shows under its title in the rail: its notes, or that it has none.
pub fn slide_notes(notes: &str) -> &str {
    if notes.is_empty() {
        "No speaker notes"
    } else {
        notes
    }
}

/// What the source of a presentation means, in the line under it, and what it is written in.
pub const HINT: &str = "Title \u{2192} appears on slide \u{b7} regular text \u{2192} your notes \u{b7} --- \u{2192} new slide";
pub const FORMAT: &str = "Markdown";

/// The slide whose lines hold `line`. The engine's `end_line` is the line after the slide's last
/// one, and it counts as the slide's own: the caret at the end of a slide is still on it.
pub fn slide_at_line(slides: &[WritingSlide], line: u32) -> Option<u32> {
    slides
        .iter()
        .find(|slide| line >= slide.start_line && line <= slide.end_line)
        .map(|slide| slide.index)
}

/// A rectangle, from the top left of the area it is in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// `size` centered in `area`.
    fn centered(size: (f32, f32), area: Rect) -> Rect {
        Rect {
            x: area.x + ((area.width - size.0) / 2.0).max(0.0),
            y: area.y + ((area.height - size.1) / 2.0).max(0.0),
            width: size.0,
            height: size.1,
        }
    }
}

/// The measures a presentation pane is laid out with (the Mac's `PaneLayoutContentMetrics`); the
/// desktop client holds them as tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub rail_width: f32,
    pub number_strip_height: f32,
    pub footer_height: f32,
    pub inset: f32,
    pub rail_divider: f32,
    pub minimum_stage: (f32, f32),
    pub maximum_stage: (f32, f32),
}

/// How the slides are offered: a rail of thumbnails beside the stage, or a strip of numbers over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    Thumbnails,
    Numbers,
}

/// Where the parts of a presentation pane are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub navigation: Navigation,
    /// The rail of thumbnails, or the strip of numbers.
    pub rail: Rect,
    /// The stage the source is in: 16:9, as large as the room left allows.
    pub stage: Rect,
    pub footer: Rect,
}

/// Lays out a pane whose body (under the header, footer included) is `body` wide and high. A body
/// smaller than the smallest stage with its margins is laid out as that one.
pub fn layout(body: (f32, f32), metrics: &Metrics) -> Layout {
    let inset = metrics.inset;
    let width = body.0.max(metrics.minimum_stage.0 + 2.0 * inset);
    let height = body.1.max(
        metrics.number_strip_height + metrics.minimum_stage.1 + metrics.footer_height + 2.0 * inset,
    );
    let above_footer = height - metrics.footer_height;
    let thumbnails =
        width >= metrics.rail_width + metrics.rail_divider + 2.0 * inset + metrics.minimum_stage.0;
    let (navigation, rail, room) = if thumbnails {
        let taken = metrics.rail_width + metrics.rail_divider;
        (
            Navigation::Thumbnails,
            Rect {
                x: 0.0,
                y: 0.0,
                width: metrics.rail_width,
                height: above_footer,
            },
            Rect {
                x: taken + inset,
                y: inset,
                width: width - taken - 2.0 * inset,
                height: above_footer - 2.0 * inset,
            },
        )
    } else {
        let strip = metrics.number_strip_height;
        (
            Navigation::Numbers,
            Rect {
                x: 0.0,
                y: 0.0,
                width,
                height: strip,
            },
            Rect {
                x: inset,
                y: strip + inset,
                width: width - 2.0 * inset,
                height: above_footer - strip - 2.0 * inset,
            },
        )
    };
    Layout {
        navigation,
        rail,
        stage: Rect::centered(stage_in(room, metrics), room),
        footer: Rect {
            x: 0.0,
            y: above_footer,
            width,
            height: metrics.footer_height,
        },
    }
}

/// The largest 16:9 size that fits `room`, no larger than the largest stage and no smaller than
/// the smallest.
fn stage_in(room: Rect, metrics: &Metrics) -> (f32, f32) {
    let (largest, smallest) = (metrics.maximum_stage, metrics.minimum_stage);
    let fits = (room.width / largest.0).min(room.height / largest.1);
    let least = (smallest.0 / largest.0).max(smallest.1 / largest.1);
    let scale = least.max(fits.min(1.0));
    (largest.0 * scale, largest.1 * scale)
}

/// What a person does to a slide show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowInput {
    RightArrow,
    LeftArrow,
    PageDown,
    PageUp,
    Space,
    Click,
    Escape,
    Other,
}

/// What a slide show does about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowCommand {
    Advance,
    Previous,
    Exit,
}

/// The command an input is, or `None` for a key the show leaves alone (any key held with the
/// platform's command key is the system's).
pub fn show_command(input: ShowInput, command_held: bool) -> Option<ShowCommand> {
    if command_held {
        return None;
    }
    match input {
        ShowInput::RightArrow | ShowInput::PageDown | ShowInput::Space | ShowInput::Click => {
            Some(ShowCommand::Advance)
        }
        ShowInput::LeftArrow | ShowInput::PageUp => Some(ShowCommand::Previous),
        ShowInput::Escape => Some(ShowCommand::Exit),
        ShowInput::Other => None,
    }
}

/// Where a slide show stands: the slide it shows among `count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Show {
    index: usize,
    count: usize,
}

impl Show {
    pub fn new(index: usize, count: usize) -> Self {
        Self {
            index: index.min(count.saturating_sub(1)),
            count,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Takes a command; true when the show must close.
    pub fn apply(&mut self, command: ShowCommand) -> bool {
        match command {
            ShowCommand::Advance => self.index = (self.index + 1).min(self.count.saturating_sub(1)),
            ShowCommand::Previous => self.index = self.index.saturating_sub(1),
            ShowCommand::Exit => return true,
        }
        false
    }

    /// The slides changed to `count`; true when none is left and the show must close.
    pub fn reconcile(&mut self, count: usize) -> bool {
        *self = Self::new(self.index, count);
        count == 0
    }
}

/// The 16:9 stage of a slide show in a viewport: the largest that fits, in the middle.
pub fn show_stage(viewport: (f32, f32)) -> Rect {
    let (width, height) = viewport;
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Rect::default();
    }
    let wide = width.min(height * SHOW_ASPECT);
    Rect::centered(
        (wide, wide / SHOW_ASPECT),
        Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
    )
}

const SHOW_ASPECT: f32 = 16.0 / 9.0;

/// How large a presented slide sets its parts, from the size of its stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideMetrics {
    pub title_size: f32,
    pub spacing: f32,
    pub padding: f32,
}

pub fn slide_metrics(stage: (f32, f32)) -> SlideMetrics {
    SlideMetrics {
        title_size: (stage.0 / 18.0).clamp(32.0, 72.0),
        spacing: (stage.1 / 18.0).clamp(18.0, 40.0),
        padding: (stage.0 / 24.0).clamp(28.0, 72.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cases, vector};
    use serde_json::Value;

    fn number(value: &Value) -> f32 {
        value.as_f64().unwrap() as f32
    }

    fn pair(value: &Value) -> (f32, f32) {
        (number(&value[0]), number(&value[1]))
    }

    fn rect(value: &Value) -> Rect {
        Rect {
            x: number(&value["x"]),
            y: number(&value["y"]),
            width: number(&value["width"]),
            height: number(&value["height"]),
        }
    }

    fn near(got: Rect, want: Rect, name: &str) {
        for (got, want) in [
            (got.x, want.x),
            (got.y, want.y),
            (got.width, want.width),
            (got.height, want.height),
        ] {
            assert!((got - want).abs() < 1e-3, "{name}: {got:?} is not {want:?}");
        }
    }

    #[test]
    fn a_slide_is_named_as_the_vector_says() {
        for case in cases("presentation", "slideTitles") {
            let heading = case["heading"].as_str().unwrap();
            assert_eq!(slide_title(heading), case["title"].as_str().unwrap());
        }
        for case in cases("presentation", "slideNotes") {
            let notes = case["notes"].as_str().unwrap();
            assert_eq!(slide_notes(notes), case["line"].as_str().unwrap());
        }
        let hint = &vector("presentation")["hint"];
        assert_eq!(HINT, hint["line"].as_str().unwrap());
        assert_eq!(FORMAT, hint["format"].as_str().unwrap());
    }

    #[test]
    fn a_line_belongs_to_the_slide_the_vector_says() {
        for case in cases("presentation", "slideAtLine") {
            let slides: Vec<WritingSlide> = case["slides"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(at, range)| WritingSlide {
                    index: u32::try_from(at).unwrap(),
                    start_line: u32::try_from(range[0].as_u64().unwrap()).unwrap(),
                    end_line: u32::try_from(range[1].as_u64().unwrap()).unwrap(),
                    heading: String::new(),
                    notes: String::new(),
                    image_paths: Vec::new(),
                })
                .collect();
            let line = u32::try_from(case["line"].as_u64().unwrap()).unwrap();
            let want = case["index"]
                .as_u64()
                .map(|index| u32::try_from(index).unwrap());
            assert_eq!(slide_at_line(&slides, line), want, "{}", case["name"]);
        }
    }

    #[test]
    fn the_show_answers_each_input_as_the_vector_says() {
        for case in cases("presentation", "showCommands") {
            let input = match case["input"].as_str().unwrap() {
                "rightArrow" => ShowInput::RightArrow,
                "leftArrow" => ShowInput::LeftArrow,
                "pageDown" => ShowInput::PageDown,
                "pageUp" => ShowInput::PageUp,
                "space" => ShowInput::Space,
                "click" => ShowInput::Click,
                "escape" => ShowInput::Escape,
                _ => ShowInput::Other,
            };
            let want = case["command"].as_str().map(|name| match name {
                "advance" => ShowCommand::Advance,
                "previous" => ShowCommand::Previous,
                _ => ShowCommand::Exit,
            });
            let modified = case["modified"].as_bool().unwrap();
            assert_eq!(show_command(input, modified), want, "{}", case["input"]);
        }
    }

    #[test]
    fn the_show_moves_as_the_vector_says() {
        for case in cases("presentation", "showStates") {
            let at = |key: &str| usize::try_from(case[key].as_u64().unwrap()).unwrap();
            let mut show = Show::new(at("index"), at("count"));
            for step in case["steps"].as_array().unwrap() {
                let exits = if let Some(count) = step["reconcile"].as_u64() {
                    show.reconcile(usize::try_from(count).unwrap())
                } else {
                    match step["apply"].as_str() {
                        Some("advance") => show.apply(ShowCommand::Advance),
                        Some("previous") => show.apply(ShowCommand::Previous),
                        Some(_) => show.apply(ShowCommand::Exit),
                        None => false,
                    }
                };
                let index = usize::try_from(step["index"].as_u64().unwrap()).unwrap();
                assert_eq!(show.index(), index, "{}", case["name"]);
                assert_eq!(exits, step["exit"].as_bool().unwrap(), "{}", case["name"]);
            }
        }
    }

    #[test]
    fn the_stage_and_the_slide_sizes_are_the_vectors_own() {
        for case in cases("presentation", "showStage") {
            let got = show_stage(pair(&case["viewport"]));
            near(got, rect(&case["stage"]), &case["viewport"].to_string());
        }
        for case in cases("presentation", "slideMetrics") {
            let got = slide_metrics(pair(&case["stage"]));
            assert!((got.title_size - number(&case["titleSize"])).abs() < 1e-3);
            assert!((got.spacing - number(&case["spacing"])).abs() < 1e-3);
            assert!((got.padding - number(&case["padding"])).abs() < 1e-3);
        }
    }

    /// The Mac's measures, as the vector holds them.
    fn vector_metrics() -> Metrics {
        let metrics = &vector("presentation")["layout"]["metrics"];
        Metrics {
            rail_width: number(&metrics["railWidth"]),
            number_strip_height: number(&metrics["numberStripHeight"]),
            footer_height: number(&metrics["footerHeight"]),
            inset: number(&metrics["inset"]),
            rail_divider: number(&metrics["railDivider"]),
            minimum_stage: pair(&metrics["minimumStage"]),
            maximum_stage: pair(&metrics["maximumStage"]),
        }
    }

    #[test]
    fn the_pane_is_laid_out_as_the_vector_says() {
        let metrics = vector_metrics();
        let cases = vector("presentation")["layout"]["cases"].clone();
        for case in cases.as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let got = layout(pair(&case["body"]), &metrics);
            let navigation = match got.navigation {
                Navigation::Thumbnails => "thumbnails",
                Navigation::Numbers => "numbers",
            };
            assert_eq!(navigation, case["navigation"].as_str().unwrap(), "{name}");
            near(got.stage, rect(&case["stage"]), name);
            near(got.footer, rect(&case["footer"]), name);
        }
    }
}
