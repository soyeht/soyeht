use crate::{Error, Result, SyntaxDocument};
use soyeht_protocol::{EditorLine, HighlightSpan};
use std::collections::BTreeMap;

pub fn project(
    document: &SyntaxDocument,
    from_line: usize,
    line_count: usize,
) -> Result<Vec<EditorLine>> {
    project_bounded(
        document,
        from_line,
        line_count,
        soyeht_protocol::limits::JSON_BYTES.saturating_sub(4096),
    )
}

pub fn project_bounded(
    document: &SyntaxDocument,
    from_line: usize,
    line_count: usize,
    byte_budget: usize,
) -> Result<Vec<EditorLine>> {
    if line_count == 0 || line_count > soyeht_protocol::limits::EDITOR_VIEWPORT_LINES {
        return Err(Error::InvalidRange);
    }
    let end = from_line
        .saturating_add(line_count)
        .min(document.buffer().line_count());
    if from_line >= end {
        return Err(Error::InvalidPosition);
    }
    let mut highlights: BTreeMap<usize, Vec<HighlightSpan>> = BTreeMap::new();
    for (line, span) in document.highlights(from_line, line_count)? {
        highlights.entry(line).or_default().push(span);
    }
    let mut output = Vec::with_capacity(end - from_line);
    let mut used: usize = 0;
    for line in from_line..end {
        let text = document.buffer().line(line)?;
        let spans = highlights.remove(&line).unwrap_or_default();
        let cost = text
            .len()
            .saturating_add(spans.len().saturating_mul(48))
            .saturating_add(64);
        if output.is_empty() && cost > byte_budget {
            return Err(Error::ViewportTooLarge);
        }
        if used.saturating_add(cost) > byte_budget {
            break;
        }
        used += cost;
        output.push(EditorLine {
            line: line as u32,
            text,
            highlights: spans,
        });
    }
    Ok(output)
}
