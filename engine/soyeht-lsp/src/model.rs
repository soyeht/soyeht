use crate::{Error, Result};
use serde_json::Value;
use soyeht_protocol::{DiagnosticSeverity, FileSymbolKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: FileSymbolKind,
    pub range: Range,
    pub selection_range: Range,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range,
    pub text: String,
}

pub(crate) fn position(value: &Value) -> Result<Position> {
    Ok(Position {
        line: number(value, "line")?,
        character: number(value, "character")?,
    })
}

pub(crate) fn range(value: &Value) -> Result<Range> {
    Ok(Range {
        start: position(
            value
                .get("start")
                .ok_or(Error::InvalidPayload("range start"))?,
        )?,
        end: position(value.get("end").ok_or(Error::InvalidPayload("range end"))?)?,
    })
}

pub(crate) fn location(value: &Value) -> Result<Location> {
    Ok(Location {
        uri: text(value, "uri", 4096)?,
        range: range(
            value
                .get("range")
                .ok_or(Error::InvalidPayload("location range"))?,
        )?,
    })
}

pub(crate) fn diagnostic(value: &Value) -> Result<Diagnostic> {
    let severity = match value.get("severity").and_then(Value::as_u64).unwrap_or(3) {
        1 => DiagnosticSeverity::Error,
        2 => DiagnosticSeverity::Warning,
        3 => DiagnosticSeverity::Information,
        4 => DiagnosticSeverity::Hint,
        _ => return Err(Error::InvalidPayload("diagnostic severity")),
    };
    Ok(Diagnostic {
        range: range(
            value
                .get("range")
                .ok_or(Error::InvalidPayload("diagnostic range"))?,
        )?,
        severity,
        message: text(value, "message", 4096)?,
        source: optional_text(value, "source", 128)?.unwrap_or_default(),
    })
}

pub(crate) fn symbol(value: &Value) -> Result<Symbol> {
    let kind = match value.get("kind").and_then(Value::as_u64) {
        Some(2) => FileSymbolKind::Module,
        Some(5) | Some(23) => FileSymbolKind::Type,
        Some(6) => FileSymbolKind::Method,
        Some(7) | Some(8) => FileSymbolKind::Property,
        Some(12) => FileSymbolKind::Function,
        Some(13) | Some(14) => FileSymbolKind::Variable,
        _ => return Err(Error::InvalidPayload("symbol kind")),
    };
    Ok(Symbol {
        name: text(value, "name", 512)?,
        kind,
        range: range(
            value
                .get("range")
                .ok_or(Error::InvalidPayload("symbol range"))?,
        )?,
        selection_range: range(
            value
                .get("selectionRange")
                .ok_or(Error::InvalidPayload("symbol selection range"))?,
        )?,
    })
}

pub(crate) fn edit(value: &Value) -> Result<TextEdit> {
    Ok(TextEdit {
        range: range(
            value
                .get("range")
                .ok_or(Error::InvalidPayload("edit range"))?,
        )?,
        text: text(value, "newText", 262_144)?,
    })
}

fn number(value: &Value, key: &'static str) -> Result<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(Error::InvalidPayload(key))
}

fn text(value: &Value, key: &'static str, max: usize) -> Result<String> {
    optional_text(value, key, max)?.ok_or(Error::InvalidPayload(key))
}

fn optional_text(value: &Value, key: &'static str, max: usize) -> Result<Option<String>> {
    let Some(value) = value.get(key) else {
        return Ok(None);
    };
    let value = value.as_str().ok_or(Error::InvalidPayload(key))?;
    if value.len() > max {
        return Err(Error::InvalidPayload(key));
    }
    Ok(Some(value.to_owned()))
}
