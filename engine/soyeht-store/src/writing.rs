//! Named persistence operations for writing documents and their bounded sidecars.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use soyeht_protocol::{
    BeatCard, FileAuthor, WritingFont, WritingKind, WritingLineVariants, WritingSection, limits,
};
use std::collections::HashSet;

const SIDECAR_HISTORY_COUNT: usize = 200;
const SIDECAR_HISTORY_BYTES: usize = 50 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WritingSidecar {
    pub line_variants: Vec<WritingLineVariants>,
    pub beat_cards: Vec<BeatCard>,
    pub scene_annotations: Vec<WritingSceneAnnotation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WritingSceneAnnotation {
    pub scene_id: String,
    pub reference_image: Option<String>,
    pub caption: Option<String>,
    pub prompt_idea: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WritingDocumentRecord {
    pub file_id: String,
    pub kind: WritingKind,
    pub section: WritingSection,
    pub sidecar_revision: u64,
    pub font: WritingFont,
    pub focus_mode: bool,
    pub sidecar: WritingSidecar,
    pub updated_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WritingSidecarVersion {
    pub file_id: String,
    pub revision: u64,
    pub author: FileAuthor,
    pub sidecar: WritingSidecar,
    pub created_ms: i64,
}

impl Store {
    pub fn writing_document(&self, file_id: &str) -> Result<Option<WritingDocumentRecord>> {
        document_from(self.connection(), file_id)
    }

    pub fn writing_documents(&self) -> Result<Vec<WritingDocumentRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT file_id,kind,section,sidecar_revision,font,focus_mode,sidecar_payload,updated_ms FROM writing_documents ORDER BY file_id",
        )?;
        let rows = statement.query_map([], decode_document)?;
        rows.map(|row| validate_document(row?)).collect()
    }

    pub fn writing_sidecar_versions(&self, file_id: &str) -> Result<Vec<WritingSidecarVersion>> {
        let mut statement = self.connection.prepare(
            "SELECT revision,author,payload,created_ms FROM writing_sidecar_versions WHERE file_id=?1 ORDER BY revision DESC",
        )?;
        let rows = statement.query_map([file_id], |row| {
            let author: String = row.get(1)?;
            let payload: String = row.get(2)?;
            Ok(WritingSidecarVersion {
                file_id: file_id.to_owned(),
                revision: u64::try_from(row.get::<_, i64>(0)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Integer,
                        Box::new(error),
                    )
                })?,
                author: serde_json::from_str(&author).map_err(json_error)?,
                sidecar: serde_json::from_str(&payload).map_err(json_error)?,
                created_ms: row.get(3)?,
            })
        })?;
        rows.map(|row| validate_sidecar_version(row?)).collect()
    }
}

impl Change<'_> {
    pub fn save_writing_document(&self, document: &WritingDocumentRecord) -> Result<()> {
        validate_document(document.clone())?;
        let payload = sidecar_json(&document.sidecar)?;
        let current = document_from(self.connection(), &document.file_id)?;
        if let Some(current) = current {
            if current.kind != document.kind
                || current.section != document.section
                || current.sidecar_revision != document.sidecar_revision
                || current.sidecar != document.sidecar
            {
                return Err(Error::InvalidWritingDocument);
            }
            self.transaction.execute(
                "UPDATE writing_documents SET font=?1,focus_mode=?2,updated_ms=?3 WHERE file_id=?4",
                params![
                    font_name(&document.font),
                    document.focus_mode,
                    document.updated_ms,
                    document.file_id
                ],
            )?;
        } else {
            if document.sidecar_revision != 0 || document.sidecar != WritingSidecar::default() {
                return Err(Error::InvalidWritingDocument);
            }
            self.transaction.execute(
                "INSERT INTO writing_documents(file_id,kind,section,sidecar_revision,font,focus_mode,sidecar_payload,updated_ms) VALUES (?1,?2,?3,0,?4,?5,?6,?7)",
                params![document.file_id, kind_name(&document.kind), section_name(&document.section), font_name(&document.font), document.focus_mode, payload, document.updated_ms],
            )?;
        }
        Ok(())
    }

    pub fn advance_writing_sidecar(
        &self,
        expected_revision: u64,
        document: &WritingDocumentRecord,
        author: &FileAuthor,
        created_ms: i64,
    ) -> Result<()> {
        validate_document(document.clone())?;
        if created_ms < 0 || document.sidecar_revision != expected_revision.saturating_add(1) {
            return Err(Error::InvalidWritingSidecar);
        }
        let current = document_from(self.connection(), &document.file_id)?
            .ok_or(Error::InvalidWritingDocument)?;
        if current.kind != document.kind
            || current.section != document.section
            || current.sidecar_revision != expected_revision
        {
            return Err(Error::InvalidWritingSidecar);
        }
        let payload = sidecar_json(&document.sidecar)?;
        let author = serde_json::to_string(author)?;
        self.transaction.execute(
            "INSERT INTO writing_sidecar_versions(file_id,revision,author,payload,created_ms) VALUES (?1,?2,?3,?4,?5)",
            params![document.file_id, to_i64(document.sidecar_revision)?, author, payload, created_ms],
        )?;
        let changed = self.transaction.execute(
            "UPDATE writing_documents SET sidecar_revision=?1,font=?2,focus_mode=?3,sidecar_payload=?4,updated_ms=?5 WHERE file_id=?6 AND sidecar_revision=?7",
            params![to_i64(document.sidecar_revision)?, font_name(&document.font), document.focus_mode, payload, document.updated_ms, document.file_id, to_i64(expected_revision)?],
        )?;
        if changed != 1 {
            return Err(Error::InvalidWritingSidecar);
        }
        prune_sidecar_history(self, &document.file_id)
    }
}

fn document_from(
    connection: &rusqlite::Connection,
    file_id: &str,
) -> Result<Option<WritingDocumentRecord>> {
    let document = connection
        .query_row(
            "SELECT file_id,kind,section,sidecar_revision,font,focus_mode,sidecar_payload,updated_ms FROM writing_documents WHERE file_id=?1",
            [file_id],
            decode_document,
        )
        .optional()?;
    document.map(validate_document).transpose()
}

fn decode_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<WritingDocumentRecord> {
    let sidecar: String = row.get(6)?;
    Ok(WritingDocumentRecord {
        file_id: row.get(0)?,
        kind: kind_from_name(&row.get::<_, String>(1)?).ok_or_else(invalid_column)?,
        section: section_from_name(&row.get::<_, String>(2)?).ok_or_else(invalid_column)?,
        sidecar_revision: u64::try_from(row.get::<_, i64>(3)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?,
        font: font_from_name(&row.get::<_, String>(4)?).ok_or_else(invalid_column)?,
        focus_mode: row.get(5)?,
        sidecar: serde_json::from_str(&sidecar).map_err(json_error)?,
        updated_ms: row.get(7)?,
    })
}

fn validate_document(document: WritingDocumentRecord) -> Result<WritingDocumentRecord> {
    let matching = matches!(
        (&document.kind, &document.section),
        (WritingKind::Text, WritingSection::Texts)
            | (WritingKind::Presentation, WritingSection::Presentations)
            | (WritingKind::Screenplay, WritingSection::Screenplays)
    );
    if document.file_id.is_empty()
        || document.file_id.len() > 128
        || document.updated_ms < 0
        || !matching
    {
        return Err(Error::InvalidWritingDocument);
    }
    validate_sidecar(&document.sidecar, &document.kind)?;
    Ok(document)
}

fn validate_sidecar(sidecar: &WritingSidecar, kind: &WritingKind) -> Result<()> {
    if !matches!(kind, WritingKind::Screenplay) && *sidecar != WritingSidecar::default() {
        return Err(Error::InvalidWritingSidecar);
    }
    if sidecar.line_variants.len() > limits::WRITING_ELEMENTS
        || sidecar.beat_cards.len() > limits::WRITING_BEAT_CARDS
        || sidecar.scene_annotations.len() > limits::WRITING_SCENES
        || sidecar
            .beat_cards
            .iter()
            .filter(|card| card.scene_id.is_some())
            .count()
            > limits::WRITING_SCENE_LINKS
    {
        return Err(Error::InvalidWritingSidecar);
    }
    let mut line_ids = HashSet::new();
    let mut variant_ids = HashSet::new();
    for variants in &sidecar.line_variants {
        if !line_ids.insert(variants.line_id.as_str())
            || variants.line_id.is_empty()
            || variants.line_id.len() > 128
            || variants.alternates.is_empty()
            || variants.alternates.len() > limits::WRITING_VARIANTS_PER_LINE
            || variants.selected_index as usize >= variants.alternates.len()
        {
            return Err(Error::InvalidWritingSidecar);
        }
        for variant in &variants.alternates {
            if !variant_ids.insert(variant.variant_id.as_str())
                || variant.variant_id.is_empty()
                || variant.variant_id.len() > 128
                || variant.text.is_empty()
                || variant.text.len() > limits::WRITING_VARIANT_BYTES
                || variant.text.contains('\n')
            {
                return Err(Error::InvalidWritingSidecar);
            }
        }
    }
    validate_beats(&sidecar.beat_cards)?;
    validate_annotations(&sidecar.scene_annotations)?;
    sidecar_json(sidecar).map(|_| ())
}

fn validate_beats(cards: &[BeatCard]) -> Result<()> {
    let mut ids = HashSet::new();
    for (index, card) in cards.iter().enumerate() {
        if !ids.insert(card.beat_id.as_str())
            || card.beat_id.is_empty()
            || card.beat_id.len() > 128
            || card.title.is_empty()
            || card.title.len() > 512
            || card.body.len() > limits::WRITING_VARIANT_BYTES
            || card.order as usize != index
            || card
                .scene_id
                .as_ref()
                .is_some_and(|scene| scene.is_empty() || scene.len() > 128)
        {
            return Err(Error::InvalidWritingSidecar);
        }
    }
    Ok(())
}

fn validate_annotations(items: &[WritingSceneAnnotation]) -> Result<()> {
    let mut ids = HashSet::new();
    for item in items {
        if !ids.insert(item.scene_id.as_str())
            || item.scene_id.is_empty()
            || item.scene_id.len() > 128
            || item
                .reference_image
                .as_ref()
                .is_some_and(|value| value.len() > 4096)
            || item
                .caption
                .as_ref()
                .is_some_and(|value| value.len() > limits::WRITING_VARIANT_BYTES)
            || item
                .prompt_idea
                .as_ref()
                .is_some_and(|value| value.len() > limits::WRITING_VARIANT_BYTES)
        {
            return Err(Error::InvalidWritingSidecar);
        }
    }
    Ok(())
}

fn validate_sidecar_version(version: WritingSidecarVersion) -> Result<WritingSidecarVersion> {
    if version.file_id.is_empty()
        || version.file_id.len() > 128
        || version.revision == 0
        || version.created_ms < 0
    {
        return Err(Error::InvalidWritingSidecar);
    }
    sidecar_json(&version.sidecar)?;
    Ok(version)
}

fn prune_sidecar_history(change: &Change<'_>, file_id: &str) -> Result<()> {
    let mut statement = change.transaction.prepare(
        "SELECT revision,length(CAST(payload AS BLOB)) FROM writing_sidecar_versions WHERE file_id=?1 ORDER BY revision DESC",
    )?;
    let rows = statement
        .query_map([file_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut bytes = 0usize;
    let mut delete = Vec::new();
    for (index, (revision, size)) in rows.into_iter().enumerate() {
        let size = usize::try_from(size).map_err(|_| Error::InvalidWritingSidecar)?;
        bytes = bytes.saturating_add(size);
        if index >= SIDECAR_HISTORY_COUNT || bytes > SIDECAR_HISTORY_BYTES {
            delete.push(revision);
        }
    }
    drop(statement);
    for revision in delete {
        change.transaction.execute(
            "DELETE FROM writing_sidecar_versions WHERE file_id=?1 AND revision=?2",
            params![file_id, revision],
        )?;
    }
    Ok(())
}

fn sidecar_json(sidecar: &WritingSidecar) -> Result<String> {
    let payload = serde_json::to_string(sidecar)?;
    if payload.len() > limits::WRITING_SIDECAR_BYTES {
        return Err(Error::InvalidWritingSidecar);
    }
    Ok(payload)
}

fn to_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::InvalidWritingSidecar)
}

fn invalid_column() -> rusqlite::Error {
    rusqlite::Error::InvalidColumnType(0, "writing enum".into(), rusqlite::types::Type::Text)
}

fn json_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn kind_name(kind: &WritingKind) -> &'static str {
    match kind {
        WritingKind::Text => "text",
        WritingKind::Presentation => "presentation",
        WritingKind::Screenplay => "screenplay",
    }
}

fn kind_from_name(value: &str) -> Option<WritingKind> {
    match value {
        "text" => Some(WritingKind::Text),
        "presentation" => Some(WritingKind::Presentation),
        "screenplay" => Some(WritingKind::Screenplay),
        _ => None,
    }
}

fn section_name(section: &WritingSection) -> &'static str {
    match section {
        WritingSection::Texts => "texts",
        WritingSection::Presentations => "presentations",
        WritingSection::Screenplays => "screenplays",
    }
}

fn section_from_name(value: &str) -> Option<WritingSection> {
    match value {
        "texts" => Some(WritingSection::Texts),
        "presentations" => Some(WritingSection::Presentations),
        "screenplays" => Some(WritingSection::Screenplays),
        _ => None,
    }
}

fn font_name(font: &WritingFont) -> &'static str {
    match font {
        WritingFont::ReadingSerif => "reading_serif",
        WritingFont::ClassicSerif => "classic_serif",
        WritingFont::Sans => "sans",
    }
}

fn font_from_name(value: &str) -> Option<WritingFont> {
    match value {
        "reading_serif" => Some(WritingFont::ReadingSerif),
        "classic_serif" => Some(WritingFont::ClassicSerif),
        "sans" => Some(WritingFont::Sans),
        _ => None,
    }
}
