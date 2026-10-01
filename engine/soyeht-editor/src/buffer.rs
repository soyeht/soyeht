use crate::{Error, Result};
use soyeht_protocol::{TextEdit, TextPosition, TextRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Original,
    Added,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Piece {
    source: Source,
    start: usize,
    len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedEdit {
    pub start_byte: usize,
    pub start_column_bytes: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_line: usize,
    pub old_end_line: usize,
    pub old_end_column_bytes: usize,
    pub new_end_line: usize,
    pub new_end_column_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct TextBuffer {
    original: String,
    added: String,
    pieces: Vec<Piece>,
    line_starts: Vec<usize>,
    revision: u64,
}

impl TextBuffer {
    pub fn new(text: impl Into<String>) -> Self {
        let original = text.into();
        let pieces = (!original.is_empty())
            .then_some(Piece {
                source: Source::Original,
                start: 0,
                len: original.len(),
            })
            .into_iter()
            .collect();
        let line_starts = line_starts(&original);
        Self {
            original,
            added: String::new(),
            pieces,
            line_starts,
            revision: 0,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn byte_len(&self) -> usize {
        self.pieces.iter().map(|piece| piece.len).sum()
    }
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn text(&self) -> String {
        let mut text = String::with_capacity(self.byte_len());
        for piece in &self.pieces {
            text.push_str(self.piece_text(*piece));
        }
        text
    }

    pub fn line(&self, line: usize) -> Result<String> {
        let start = *self.line_starts.get(line).ok_or(Error::InvalidPosition)?;
        let end = self.line_content_end(line);
        Ok(self.bytes(start, end))
    }

    pub fn byte_at(&self, position: &TextPosition) -> Result<usize> {
        let line = usize::try_from(position.line).map_err(|_| Error::InvalidPosition)?;
        let utf16 = usize::try_from(position.column).map_err(|_| Error::InvalidPosition)?;
        let start = *self.line_starts.get(line).ok_or(Error::InvalidPosition)?;
        let text = self.bytes(start, self.line_content_end(line));
        let mut units = 0;
        for (offset, character) in text.char_indices() {
            if units == utf16 {
                return Ok(start + offset);
            }
            units += character.len_utf16();
            if units > utf16 {
                return Err(Error::InvalidPosition);
            }
        }
        (units == utf16)
            .then_some(start + text.len())
            .ok_or(Error::InvalidPosition)
    }

    pub fn point_at_byte(&self, byte: usize) -> Result<(usize, usize)> {
        if byte > self.byte_len() {
            return Err(Error::InvalidPosition);
        }
        let line = self
            .line_starts
            .partition_point(|start| *start <= byte)
            .saturating_sub(1);
        Ok((line, byte - self.line_starts[line]))
    }

    pub fn apply(&mut self, edit: &TextEdit) -> Result<AppliedEdit> {
        let mut applied = self.apply_all(std::slice::from_ref(edit))?;
        applied.pop().ok_or(Error::InvalidRange)
    }

    pub fn apply_all(&mut self, edits: &[TextEdit]) -> Result<Vec<AppliedEdit>> {
        self.apply_within(edits, soyeht_protocol::limits::EDITOR_EDIT_BYTES)
    }

    /// Applies edits the engine produced itself, such as an undo step, whose
    /// restored text is bounded by the document size rather than one wire edit.
    pub(crate) fn apply_restoring(&mut self, edits: &[TextEdit]) -> Result<Vec<AppliedEdit>> {
        self.apply_within(edits, soyeht_protocol::limits::EDITOR_HISTORY_BYTES)
    }

    /// The UTF-16 position of a byte offset that lies on a scalar boundary.
    pub fn position_at_byte(&self, byte: usize) -> Result<TextPosition> {
        let (line, column) = self.point_at_byte(byte)?;
        let text = self.line(line)?;
        let prefix = text.get(..column).ok_or(Error::InvalidPosition)?;
        Ok(TextPosition {
            line: u32::try_from(line).map_err(|_| Error::InvalidPosition)?,
            column: u32::try_from(prefix.encode_utf16().count())
                .map_err(|_| Error::InvalidPosition)?,
        })
    }

    pub(crate) fn slice(&self, start: usize, end: usize) -> String {
        self.bytes(start, end)
    }

    fn apply_within(&mut self, edits: &[TextEdit], limit: usize) -> Result<Vec<AppliedEdit>> {
        let mut ranges = edits
            .iter()
            .map(|edit| {
                if edit.text.len() > limit {
                    return Err(Error::EditTooLarge);
                }
                let (start, end) = self.byte_range(&edit.range)?;
                Ok((start, end, edit))
            })
            .collect::<Result<Vec<_>>>()?;
        ranges.sort_by_key(|(start, _, _)| *start);
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(Error::OverlappingEdits);
        }
        let mut applied = Vec::with_capacity(ranges.len());
        for (_, _, edit) in ranges.into_iter().rev() {
            applied.push(self.apply_one(edit)?);
        }
        applied.reverse();
        if !applied.is_empty() {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(applied)
    }

    pub(crate) fn chunk_at(&self, byte: usize) -> &[u8] {
        let mut offset = 0;
        for piece in &self.pieces {
            let end = offset + piece.len;
            if byte < end {
                let relative = byte - offset;
                return &self.piece_text(*piece).as_bytes()[relative..];
            }
            offset = end;
        }
        &[]
    }

    fn apply_one(&mut self, edit: &TextEdit) -> Result<AppliedEdit> {
        let (start, end) = self.byte_range(&edit.range)?;
        let (start_line, start_column) = self.point_at_byte(start)?;
        let (old_end_line, old_end_column_bytes) = self.point_at_byte(end)?;
        let added_start = self.added.len();
        self.added.push_str(&edit.text);
        let mut next = self.slice_pieces(0, start);
        if !edit.text.is_empty() {
            next.push(Piece {
                source: Source::Added,
                start: added_start,
                len: edit.text.len(),
            });
        }
        next.extend(self.slice_pieces(end, self.byte_len()));
        self.pieces = merge_adjacent(next);
        self.update_lines(start, end, &edit.text);
        let (new_end_line, new_end_column_bytes) =
            replacement_end(start_line, start_column, &edit.text);
        Ok(AppliedEdit {
            start_byte: start,
            old_end_byte: end,
            new_end_byte: start + edit.text.len(),
            start_line,
            start_column_bytes: start_column,
            old_end_line,
            old_end_column_bytes,
            new_end_line,
            new_end_column_bytes,
        })
    }

    pub(crate) fn byte_range(&self, range: &TextRange) -> Result<(usize, usize)> {
        let start = self.byte_at(&range.start)?;
        let end = self.byte_at(&range.end)?;
        (start <= end)
            .then_some((start, end))
            .ok_or(Error::InvalidRange)
    }

    fn line_content_end(&self, line: usize) -> usize {
        let raw = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or_else(|| self.byte_len());
        if raw > 0 && self.byte(raw - 1) == Some(b'\n') {
            raw - 1
        } else {
            raw
        }
    }

    fn byte(&self, index: usize) -> Option<u8> {
        self.chunk_at(index).first().copied()
    }

    fn bytes(&self, start: usize, end: usize) -> String {
        let mut output = String::with_capacity(end.saturating_sub(start));
        let mut offset = 0;
        for piece in &self.pieces {
            let piece_end = offset + piece.len;
            let from = start.saturating_sub(offset).min(piece.len);
            let to = end.saturating_sub(offset).min(piece.len);
            if from < to {
                output.push_str(&self.piece_text(*piece)[from..to]);
            }
            if piece_end >= end {
                break;
            }
            offset = piece_end;
        }
        output
    }

    fn slice_pieces(&self, start: usize, end: usize) -> Vec<Piece> {
        let mut output = Vec::new();
        let mut offset = 0;
        for piece in &self.pieces {
            let from = start.saturating_sub(offset).min(piece.len);
            let to = end.saturating_sub(offset).min(piece.len);
            if from < to {
                output.push(Piece {
                    start: piece.start + from,
                    len: to - from,
                    ..*piece
                });
            }
            offset += piece.len;
            if offset >= end {
                break;
            }
        }
        output
    }

    fn piece_text(&self, piece: Piece) -> &str {
        let source = match piece.source {
            Source::Original => &self.original,
            Source::Added => &self.added,
        };
        &source[piece.start..piece.start + piece.len]
    }

    fn update_lines(&mut self, start: usize, end: usize, replacement: &str) {
        let delta = replacement.len() as i128 - (end - start) as i128;
        self.line_starts
            .retain(|line| *line <= start || *line > end);
        for line in &mut self.line_starts {
            if *line > end {
                *line = ((*line as i128) + delta) as usize;
            }
        }
        let inserted = replacement
            .match_indices('\n')
            .map(|(index, _)| start + index + 1);
        self.line_starts.extend(inserted);
        self.line_starts.sort_unstable();
        self.line_starts.dedup();
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
}

fn replacement_end(line: usize, column: usize, text: &str) -> (usize, usize) {
    match text.rsplit_once('\n') {
        Some((before, after)) => (
            line + before.bytes().filter(|byte| *byte == b'\n').count() + 1,
            after.len(),
        ),
        None => (line, column + text.len()),
    }
}

fn merge_adjacent(pieces: Vec<Piece>) -> Vec<Piece> {
    let mut merged: Vec<Piece> = Vec::with_capacity(pieces.len());
    for piece in pieces {
        if let Some(last) = merged.last_mut()
            && last.source == piece.source
            && last.start + last.len == piece.start
        {
            last.len += piece.len;
        } else {
            merged.push(piece);
        }
    }
    merged
}
