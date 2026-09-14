use crate::{Error, Result};
use soyeht_protocol::{WritingSlide, limits};

pub fn segment_slides(source: &str) -> Result<Vec<WritingSlide>> {
    if source.len() > limits::EDITOR_HISTORY_BYTES {
        return Err(Error::DocumentTooLarge);
    }
    let lines: Vec<&str> = source.lines().collect();
    let ranges = slide_ranges(&lines);
    if ranges.len() > limits::WRITING_SLIDES {
        return Err(Error::SlideLimit);
    }
    ranges
        .into_iter()
        .enumerate()
        .map(|(index, (start, end))| slide(&lines, index, start, end))
        .collect()
}

fn slide_ranges(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    while index < lines.len() {
        if lines[index].trim() == "---" {
            push_nonempty(lines, &mut ranges, start, index);
            start = index + 1;
            index += 1;
            continue;
        }
        if lines[index].trim().is_empty() {
            let blank_start = index;
            while index < lines.len() && lines[index].trim().is_empty() {
                index += 1;
            }
            if index.saturating_sub(blank_start) >= 2 {
                push_nonempty(lines, &mut ranges, start, blank_start);
                start = index;
            }
            continue;
        }
        index += 1;
    }
    push_nonempty(lines, &mut ranges, start, lines.len());
    if ranges.is_empty() {
        ranges.push((0, 0));
    }
    ranges
}

fn push_nonempty(lines: &[&str], ranges: &mut Vec<(usize, usize)>, start: usize, end: usize) {
    let first = (start..end).find(|index| !lines[*index].trim().is_empty());
    let last = (start..end)
        .rev()
        .find(|index| !lines[*index].trim().is_empty());
    if let (Some(first), Some(last)) = (first, last) {
        ranges.push((first, last + 1));
    }
}

fn slide(lines: &[&str], index: usize, start: usize, end: usize) -> Result<WritingSlide> {
    let heading_index = (start..end).find(|line| heading(lines[*line]).is_some());
    let heading = heading_index
        .and_then(|line| heading(lines[line]))
        .unwrap_or_default()
        .to_owned();
    let notes = (start..end)
        .filter(|line| Some(*line) != heading_index)
        .map(|line| lines[line])
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();
    if heading.len() > 4096 || notes.len() > limits::WRITING_SUGGESTION_BYTES {
        return Err(Error::DocumentTooLarge);
    }
    Ok(WritingSlide {
        index: u32::try_from(index).map_err(|_| Error::SlideLimit)?,
        start_line: u32::try_from(start).map_err(|_| Error::DocumentTooLarge)?,
        end_line: u32::try_from(end).map_err(|_| Error::DocumentTooLarge)?,
        heading,
        image_paths: image_paths(&lines[start..end]),
        notes,
    })
}

fn heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let body = trimmed.trim_start_matches('#');
    (body.len() < trimmed.len() && body.starts_with(' ')).then(|| body.trim())
}

fn image_paths(lines: &[&str]) -> Vec<String> {
    let mut paths = Vec::new();
    for line in lines {
        let mut rest = *line;
        while let Some(open) = rest.find("![") {
            rest = &rest[open + 2..];
            let Some(close_label) = rest.find("](") else {
                break;
            };
            rest = &rest[close_label + 2..];
            let Some(close_path) = rest.find(')') else {
                break;
            };
            let path = &rest[..close_path];
            if !path.is_empty() && path.len() <= 4096 {
                paths.push(path.to_owned());
            }
            rest = &rest[close_path + 1..];
        }
    }
    paths
}
