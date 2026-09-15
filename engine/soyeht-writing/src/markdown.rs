use crate::{Error, Result};
use soyeht_protocol::{TextPosition, TextRange, WritingFormatKind, limits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridBlockKind {
    Blank,
    Heading,
    Paragraph,
    ListItem,
    Quote,
    CodeFence,
    Code,
    Table,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HybridBlock {
    pub line: u32,
    pub kind: HybridBlockKind,
    pub raw_syntax: bool,
    pub text: String,
}

pub fn format_markdown(
    source: &str,
    range: &TextRange,
    kind: WritingFormatKind,
    value: Option<&str>,
) -> Result<String> {
    validate_document(source)?;
    let (start, end) = byte_range(source, range)?;
    match kind {
        WritingFormatKind::Heading1 => prefix_lines(source, start, end, "# "),
        WritingFormatKind::Heading2 => prefix_lines(source, start, end, "## "),
        WritingFormatKind::Heading3 => prefix_lines(source, start, end, "### "),
        WritingFormatKind::BulletList => prefix_lines(source, start, end, "- "),
        WritingFormatKind::NumberedList => numbered_lines(source, start, end),
        WritingFormatKind::Quote => prefix_lines(source, start, end, "> "),
        WritingFormatKind::Bold => wrap(source, start, end, "**", "**"),
        WritingFormatKind::Italic => wrap(source, start, end, "_", "_"),
        WritingFormatKind::Link => wrap_value(source, start, end, value, false),
        WritingFormatKind::Image => wrap_value(source, start, end, value, true),
    }
}

pub fn hybrid_blocks(source: &str, active_line: u32) -> Result<Vec<HybridBlock>> {
    validate_document(source)?;
    let lines: Vec<&str> = source.lines().collect();
    let mut in_fence = false;
    let mut blocks = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        let kind = block_kind(&lines, index, &mut in_fence);
        blocks.push(HybridBlock {
            line: u32::try_from(index).map_err(|_| Error::DocumentTooLarge)?,
            kind,
            raw_syntax: index == active_line as usize,
            text: visible_text(line, kind, index == active_line as usize),
        });
    }
    Ok(blocks)
}

fn validate_document(source: &str) -> Result<()> {
    if source.len() > limits::EDITOR_HISTORY_BYTES {
        return Err(Error::DocumentTooLarge);
    }
    Ok(())
}

fn byte_range(source: &str, range: &TextRange) -> Result<(usize, usize)> {
    let start = byte_at(source, &range.start)?;
    let end = byte_at(source, &range.end)?;
    if start > end {
        return Err(Error::InvalidRange);
    }
    Ok((start, end))
}

fn byte_at(source: &str, position: &TextPosition) -> Result<usize> {
    let mut offset = 0usize;
    let line = source
        .split_inclusive('\n')
        .nth(position.line as usize)
        .ok_or(Error::InvalidPosition)?;
    for prior in source.split_inclusive('\n').take(position.line as usize) {
        offset = offset
            .checked_add(prior.len())
            .ok_or(Error::InvalidPosition)?;
    }
    let content = line.strip_suffix('\n').unwrap_or(line);
    let mut utf16 = 0usize;
    for (byte, ch) in content.char_indices() {
        if utf16 == position.column as usize {
            return Ok(offset + byte);
        }
        utf16 += ch.len_utf16();
        if utf16 > position.column as usize {
            return Err(Error::InvalidRange);
        }
    }
    if utf16 == position.column as usize {
        Ok(offset + content.len())
    } else {
        Err(Error::InvalidPosition)
    }
}

fn wrap(source: &str, start: usize, end: usize, left: &str, right: &str) -> Result<String> {
    if start == end {
        return Err(Error::EmptySelection);
    }
    let mut output = String::with_capacity(source.len() + left.len() + right.len());
    output.push_str(&source[..start]);
    output.push_str(left);
    output.push_str(&source[start..end]);
    output.push_str(right);
    output.push_str(&source[end..]);
    validate_document(&output)?;
    Ok(output)
}

fn wrap_value(
    source: &str,
    start: usize,
    end: usize,
    value: Option<&str>,
    image: bool,
) -> Result<String> {
    let destination = value
        .filter(|value| !value.is_empty())
        .ok_or(Error::MissingFormatValue)?;
    if destination.len() > 4096 || start == end {
        return Err(if start == end {
            Error::EmptySelection
        } else {
            Error::MissingFormatValue
        });
    }
    let left = if image { "![" } else { "[" };
    wrap(source, start, end, left, &format!("]({destination})"))
}

fn selected_lines(source: &str, start: usize, end: usize) -> (usize, usize) {
    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[end..]
        .find('\n')
        .map_or(source.len(), |index| end + index);
    (line_start, line_end)
}

fn prefix_lines(source: &str, start: usize, end: usize, prefix: &str) -> Result<String> {
    let (line_start, line_end) = selected_lines(source, start, end);
    let selected = &source[line_start..line_end];
    let body = selected
        .lines()
        .map(|line| format!("{prefix}{}", strip_block_prefix(line)))
        .collect::<Vec<_>>()
        .join("\n");
    replace_lines(source, line_start, line_end, &body)
}

fn numbered_lines(source: &str, start: usize, end: usize) -> Result<String> {
    let (line_start, line_end) = selected_lines(source, start, end);
    let selected = &source[line_start..line_end];
    let body = selected
        .lines()
        .enumerate()
        .map(|(index, line)| format!("{}. {}", index + 1, strip_block_prefix(line)))
        .collect::<Vec<_>>()
        .join("\n");
    replace_lines(source, line_start, line_end, &body)
}

fn replace_lines(source: &str, start: usize, end: usize, body: &str) -> Result<String> {
    let mut output = String::with_capacity(source.len() + body.len());
    output.push_str(&source[..start]);
    output.push_str(body);
    output.push_str(&source[end..]);
    validate_document(&output)?;
    Ok(output)
}

fn strip_block_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    let heading = trimmed.trim_start_matches('#').strip_prefix(' ');
    if let Some(body) = heading {
        return body;
    }
    for prefix in ["> ", "- ", "* ", "+ "] {
        if let Some(body) = trimmed.strip_prefix(prefix) {
            return body;
        }
    }
    let numbered = trimmed.split_once(". ");
    if numbered.is_some_and(|(number, _)| number.chars().all(|ch| ch.is_ascii_digit())) {
        return numbered.map_or(trimmed, |(_, body)| body);
    }
    trimmed
}

fn block_kind(lines: &[&str], index: usize, in_fence: &mut bool) -> HybridBlockKind {
    let line = lines[index];
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        *in_fence = !*in_fence;
        return HybridBlockKind::CodeFence;
    }
    if *in_fence {
        return HybridBlockKind::Code;
    }
    if line.trim().is_empty() {
        return HybridBlockKind::Blank;
    }
    if trimmed.starts_with('#') && trimmed.trim_start_matches('#').starts_with(' ') {
        return HybridBlockKind::Heading;
    }
    if trimmed.starts_with("![") {
        return HybridBlockKind::Image;
    }
    if trimmed.starts_with("> ") {
        return HybridBlockKind::Quote;
    }
    if list_item(trimmed) {
        return HybridBlockKind::ListItem;
    }
    if table_line(lines, index) {
        return HybridBlockKind::Table;
    }
    HybridBlockKind::Paragraph
}

fn list_item(line: &str) -> bool {
    ["- ", "* ", "+ "]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        || line
            .split_once(". ")
            .is_some_and(|(number, _)| number.chars().all(|ch| ch.is_ascii_digit()))
}

fn table_line(lines: &[&str], index: usize) -> bool {
    if !lines[index].contains('|') {
        return false;
    }
    let mut header = index;
    while header > 0 && lines[header - 1].contains('|') {
        header -= 1;
    }
    lines.get(header + 1).is_some_and(|line| {
        let cells: Vec<_> = line
            .split('|')
            .filter(|cell| !cell.trim().is_empty())
            .collect();
        cells.len() >= 2
            && cells.iter().all(|cell| {
                let cell = cell.trim().trim_matches(':');
                cell.len() >= 3 && cell.chars().all(|ch| ch == '-')
            })
    })
}

fn visible_text(line: &str, kind: HybridBlockKind, raw: bool) -> String {
    if raw {
        return line.to_owned();
    }
    match kind {
        HybridBlockKind::Heading => line.trim_start().trim_start_matches('#').trim().to_owned(),
        HybridBlockKind::ListItem => strip_block_prefix(line).to_owned(),
        HybridBlockKind::Quote => line.trim_start().trim_start_matches("> ").to_owned(),
        HybridBlockKind::CodeFence => String::new(),
        _ => line.to_owned(),
    }
}
