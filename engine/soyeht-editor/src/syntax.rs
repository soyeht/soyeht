use crate::{AppliedEdit, Error, Result, TextBuffer};
use soyeht_protocol::{EditorLanguage, HighlightKind, HighlightSpan};
use tree_sitter::{InputEdit, Parser, Point, Tree};

pub struct SyntaxDocument {
    buffer: TextBuffer,
    syntax: Option<SyntaxTree>,
    parsed_revision: u64,
    last_changed_ranges: Vec<std::ops::Range<usize>>,
}

struct SyntaxTree {
    parser: Parser,
    tree: Tree,
}

impl SyntaxDocument {
    pub fn new(text: impl Into<String>, language: EditorLanguage) -> Result<Self> {
        let buffer = TextBuffer::new(text);
        let syntax = parser(language)?
            .map(|mut parser| {
                let tree = parse(&mut parser, &buffer, None)?;
                Ok::<_, Error>(SyntaxTree { parser, tree })
            })
            .transpose()?;
        Ok(Self {
            buffer,
            syntax,
            parsed_revision: 0,
            last_changed_ranges: Vec::new(),
        })
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }
    pub fn parsed_revision(&self) -> u64 {
        self.parsed_revision
    }
    pub fn last_changed_ranges(&self) -> &[std::ops::Range<usize>] {
        &self.last_changed_ranges
    }
    pub fn has_syntax_error(&self) -> bool {
        self.syntax
            .as_ref()
            .is_some_and(|syntax| syntax.tree.root_node().has_error())
    }

    pub fn apply(&mut self, edit: &soyeht_protocol::TextEdit) -> Result<AppliedEdit> {
        let mut applied = self.apply_all(std::slice::from_ref(edit))?;
        applied.pop().ok_or(Error::InvalidRange)
    }

    pub fn apply_all(&mut self, edits: &[soyeht_protocol::TextEdit]) -> Result<Vec<AppliedEdit>> {
        let applied = self.buffer.apply_all(edits)?;
        if let Some(syntax) = &mut self.syntax {
            for edit in applied.iter().rev() {
                syntax.tree.edit(&input_edit(edit));
            }
            let next = parse(&mut syntax.parser, &self.buffer, Some(&syntax.tree))?;
            self.last_changed_ranges = syntax
                .tree
                .changed_ranges(&next)
                .map(|range| range.start_byte..range.end_byte)
                .collect();
            syntax.tree = next;
        } else {
            self.last_changed_ranges.clear();
        }
        self.parsed_revision = self.buffer.revision();
        Ok(applied)
    }

    pub fn highlights(
        &self,
        from_line: usize,
        line_count: usize,
    ) -> Result<Vec<(usize, HighlightSpan)>> {
        let Some(syntax) = &self.syntax else {
            return Ok(Vec::new());
        };
        if from_line >= self.buffer.line_count() {
            return Err(Error::InvalidPosition);
        }
        let to_line = from_line
            .saturating_add(line_count)
            .min(self.buffer.line_count());
        let mut cursor = syntax.tree.walk();
        let mut pending = vec![syntax.tree.root_node()];
        let mut spans = Vec::new();
        while let Some(node) = pending.pop() {
            if node.start_position().row >= to_line || node.end_position().row < from_line {
                continue;
            }
            if node.child_count() == 0 {
                if let Some(kind) =
                    highlight_kind(node.kind(), node.parent().map(|parent| parent.kind()))
                {
                    push_spans(
                        &mut spans,
                        &self.buffer,
                        node.start_position(),
                        node.end_position(),
                        from_line,
                        to_line,
                        kind,
                    )?;
                }
            } else {
                cursor.reset(node);
                if cursor.goto_first_child() {
                    loop {
                        pending.push(cursor.node());
                        if !cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
        }
        spans.sort_by_key(|(line, span)| (*line, span.start_column));
        Ok(spans)
    }
}

fn parser(language: EditorLanguage) -> Result<Option<Parser>> {
    let language = match language {
        EditorLanguage::PlainText => return Ok(None),
        EditorLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        EditorLanguage::Swift => tree_sitter_swift::LANGUAGE.into(),
        EditorLanguage::Typescript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        EditorLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        EditorLanguage::Json => tree_sitter_json::LANGUAGE.into(),
        EditorLanguage::Markdown => tree_sitter_md::LANGUAGE.into(),
        EditorLanguage::Shell => tree_sitter_bash::LANGUAGE.into(),
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|_| Error::ParserLanguage)?;
    Ok(Some(parser))
}

fn parse(parser: &mut Parser, buffer: &TextBuffer, old: Option<&Tree>) -> Result<Tree> {
    parser
        .parse_with_options(&mut |byte, _| buffer.chunk_at(byte), old, None)
        .ok_or(Error::ParserStopped)
}

fn input_edit(edit: &AppliedEdit) -> InputEdit {
    InputEdit {
        start_byte: edit.start_byte,
        old_end_byte: edit.old_end_byte,
        new_end_byte: edit.new_end_byte,
        start_position: Point::new(edit.start_line, edit.start_column_bytes),
        old_end_position: Point::new(edit.old_end_line, edit.old_end_column_bytes),
        new_end_position: Point::new(edit.new_end_line, edit.new_end_column_bytes),
    }
}

fn highlight_kind(kind: &str, parent: Option<&str>) -> Option<HighlightKind> {
    if kind.contains("comment") {
        return Some(HighlightKind::Comment);
    }
    if kind.contains("string") || kind.contains("character") {
        return Some(HighlightKind::String);
    }
    if kind.contains("number") || kind.contains("integer") || kind.contains("float") {
        return Some(HighlightKind::Number);
    }
    if kind.contains("type_identifier") || kind == "primitive_type" {
        return Some(HighlightKind::Type);
    }
    if kind.contains("property_identifier") || kind.contains("field_identifier") {
        return Some(HighlightKind::Property);
    }
    if kind == "identifier" && parent.is_some_and(|value| value.contains("function")) {
        return Some(HighlightKind::Function);
    }
    if KEYWORDS.contains(&kind) {
        return Some(HighlightKind::Keyword);
    }
    if OPERATORS.contains(&kind) {
        return Some(HighlightKind::Operator);
    }
    None
}

fn push_spans(
    output: &mut Vec<(usize, HighlightSpan)>,
    buffer: &TextBuffer,
    start: Point,
    end: Point,
    from: usize,
    to: usize,
    kind: HighlightKind,
) -> Result<()> {
    let first = start.row.max(from);
    let last = end.row.min(to.saturating_sub(1));
    for line in first..=last {
        let start_column = if line == start.row {
            utf16_column(buffer, line, start.column)?
        } else {
            0
        };
        let end_column = if line == end.row {
            utf16_column(buffer, line, end.column)?
        } else {
            buffer.line(line)?.encode_utf16().count()
        };
        output.push((
            line,
            HighlightSpan {
                start_column: start_column as u32,
                end_column: end_column as u32,
                kind: kind.clone(),
            },
        ));
    }
    Ok(())
}

fn utf16_column(buffer: &TextBuffer, line: usize, byte_column: usize) -> Result<usize> {
    let text = buffer.line(line)?;
    let prefix = text.get(..byte_column).ok_or(Error::InvalidPosition)?;
    Ok(prefix.encode_utf16().count())
}

const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "mut",
    "pub",
    "struct",
    "enum",
    "impl",
    "trait",
    "use",
    "mod",
    "return",
    "if",
    "else",
    "for",
    "while",
    "match",
    "async",
    "await",
    "func",
    "var",
    "class",
    "import",
    "def",
    "from",
    "const",
    "function",
    "interface",
    "type",
    "true",
    "false",
    "null",
];
const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "=", "==", "!=", "=>", "->", "<", ">", "<=", ">=",
];
