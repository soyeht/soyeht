use crate::{Error, Result};
use soyeht_protocol::{ScreenplayElement, ScreenplayElementKind, ScreenplayScene, limits};

#[derive(Debug, Clone, PartialEq)]
pub struct Screenplay {
    pub elements: Vec<ScreenplayElement>,
    pub scenes: Vec<ScreenplayScene>,
}

pub fn parse_screenplay(source: &str) -> Result<Screenplay> {
    if source.len() > limits::EDITOR_HISTORY_BYTES {
        return Err(Error::DocumentTooLarge);
    }
    let lines: Vec<&str> = source.lines().collect();
    let mut elements = Vec::new();
    let mut scenes = Vec::new();
    let mut current_scene = None;
    let mut previous_kind = None;
    for (line_index, raw) in lines.iter().enumerate() {
        let text = raw.trim();
        if text.is_empty() {
            if !matches!(
                previous_kind,
                Some(ScreenplayElementKind::Character | ScreenplayElementKind::Parenthetical)
            ) {
                previous_kind = None;
            }
            continue;
        }
        let kind = classify(
            text,
            previous_kind.as_ref(),
            next_nonempty(&lines, line_index + 1),
        );
        let line = u32::try_from(line_index).map_err(|_| Error::DocumentTooLarge)?;
        if kind == ScreenplayElementKind::SceneHeading {
            let scene_id = format!("scene-{}-{line}", scenes.len());
            scenes.push(scene(scene_id.clone(), scenes.len(), line, text)?);
            current_scene = Some(scene_id);
        }
        elements.push(ScreenplayElement {
            element_id: format!("element-{line}"),
            kind: kind.clone(),
            start_line: line,
            end_line: line,
            text: display_text(text, &kind).to_owned(),
            scene_id: current_scene.clone(),
        });
        if elements.len() > limits::WRITING_ELEMENTS {
            return Err(Error::ElementLimit);
        }
        previous_kind = Some(kind);
    }
    if scenes.len() > limits::WRITING_SCENES {
        return Err(Error::SceneLimit);
    }
    Ok(Screenplay { elements, scenes })
}

pub fn advance_after(kind: &ScreenplayElementKind, tab: bool) -> ScreenplayElementKind {
    match (kind, tab) {
        (ScreenplayElementKind::SceneHeading, _) => ScreenplayElementKind::Action,
        (ScreenplayElementKind::Character, _) | (ScreenplayElementKind::Parenthetical, _) => {
            ScreenplayElementKind::Dialogue
        }
        (ScreenplayElementKind::Dialogue, true) => ScreenplayElementKind::Parenthetical,
        (ScreenplayElementKind::Dialogue, false) => ScreenplayElementKind::Character,
        (ScreenplayElementKind::Action, true) => ScreenplayElementKind::Character,
        (ScreenplayElementKind::Transition, _) | (ScreenplayElementKind::Shot, _) => {
            ScreenplayElementKind::SceneHeading
        }
        _ => ScreenplayElementKind::Action,
    }
}

fn classify(
    text: &str,
    previous: Option<&ScreenplayElementKind>,
    next: Option<&str>,
) -> ScreenplayElementKind {
    if scene_heading(text) {
        ScreenplayElementKind::SceneHeading
    } else if transition(text) {
        ScreenplayElementKind::Transition
    } else if text.starts_with("[action]") {
        ScreenplayElementKind::Action
    } else if text.starts_with('(') && text.ends_with(')') {
        ScreenplayElementKind::Parenthetical
    } else if colon_character(text, next) || character(text, next) {
        ScreenplayElementKind::Character
    } else if matches!(
        previous,
        Some(
            ScreenplayElementKind::Character
                | ScreenplayElementKind::Parenthetical
                | ScreenplayElementKind::Dialogue
        )
    ) {
        ScreenplayElementKind::Dialogue
    } else if shot(text) {
        ScreenplayElementKind::Shot
    } else {
        ScreenplayElementKind::Action
    }
}

fn scene_heading(text: &str) -> bool {
    let upper = text.trim_start_matches('.').to_ascii_uppercase();
    ["INT.", "EXT.", "INT/EXT.", "EXT/INT.", "I/E.", "EST."]
        .iter()
        .any(|prefix| upper.starts_with(prefix))
}

fn transition(text: &str) -> bool {
    let clean = text.trim_matches('>');
    text.starts_with('>')
        || (clean == clean.to_ascii_uppercase()
            && (clean.ends_with(" TO:") || clean.starts_with("FADE ")))
}

fn character(text: &str, next: Option<&str>) -> bool {
    let clean = text.trim_start_matches('@');
    let forced = text.starts_with('@');
    !clean.is_empty()
        && clean.len() <= 80
        && clean == clean.to_ascii_uppercase()
        && (forced || next.is_some())
        && !clean.ends_with(':')
        && !scene_heading(clean)
}

fn colon_character(text: &str, next: Option<&str>) -> bool {
    let Some(clean) = text.strip_suffix(':') else {
        return false;
    };
    let clean = clean.trim_start_matches('@').trim();
    let name = clean
        .split_once(" (")
        .map_or(clean, |(name, _)| name)
        .trim();
    next.is_some()
        && !name.is_empty()
        && name.len() <= 80
        && name == name.to_ascii_uppercase()
        && name
            .chars()
            .all(|value| value.is_alphanumeric() || " .'-".contains(value))
}

fn shot(text: &str) -> bool {
    text.len() <= 80 && text == text.to_ascii_uppercase() && text.ends_with(':')
}

fn next_nonempty<'a>(lines: &'a [&str], start: usize) -> Option<&'a str> {
    lines[start..]
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
}

fn display_text<'a>(text: &'a str, kind: &ScreenplayElementKind) -> &'a str {
    match kind {
        ScreenplayElementKind::SceneHeading => text.trim_start_matches('.'),
        ScreenplayElementKind::Character => text.trim_start_matches('@').trim_end_matches(':'),
        ScreenplayElementKind::Transition => text.trim_matches('>'),
        ScreenplayElementKind::Action => {
            text.strip_prefix("[action]").map_or(text, str::trim_start)
        }
        _ => text,
    }
}

fn scene(scene_id: String, ordinal: usize, line: u32, heading: &str) -> Result<ScreenplayScene> {
    Ok(ScreenplayScene {
        scene_id,
        ordinal: u32::try_from(ordinal).map_err(|_| Error::SceneLimit)?,
        heading: heading.trim_start_matches('.').to_owned(),
        line,
        reference_image: None,
        caption: None,
        prompt_idea: None,
        linked_beat_ids: Vec::new(),
    })
}
