use super::documents::{public_document, require_revision};
use crate::{core::Core, errors, errors::Result};
use serde_json::Value;
use soyeht_protocol::{
    ScreenplayScene, WritingKind, WritingSectionListParams, WritingSectionListResult,
    WritingStructureParams, WritingStructureResult, limits,
};
use soyeht_writing::SectionEntryInput;
use std::path::Path;

impl Core {
    pub(super) fn writing_section_list(&self, params: &Value) -> Result<Value> {
        let request: WritingSectionListParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid writing section list parameters"))?;
        if request.limit == 0 || request.limit as usize > limits::WRITING_SECTION_ENTRIES {
            return Err(errors::invalid("Invalid writing section page limit"));
        }
        let workspace = crate::model::workspace(&self.store, &request.workspace_id)?;
        let folder = soyeht_writing::folder_for(&request.section);
        let mut inputs = Vec::new();
        for file in self.editor.project_files(Path::new(&workspace.path))? {
            if !Path::new(&file.relative).starts_with(folder) {
                continue;
            }
            let opened = self
                .store
                .editor_file_for_path(&request.workspace_id, &file.relative)
                .map_err(errors::storage)?;
            inputs.push(SectionEntryInput {
                path: file.relative.clone(),
                title: title(&file.relative),
                file_id: opened.map(|item| item.file_id),
                modified_at: timestamp(file.modified_ms),
                byte_count: file.byte_count,
            });
            if inputs.len() > limits::WRITING_SECTION_ENTRIES {
                return Err(errors::Error::Writing(soyeht_writing::Error::SectionLimit));
            }
        }
        let entries = soyeht_writing::section_entries(request.section, inputs)?;
        let start = cursor_start(&entries, request.cursor.as_deref())?;
        let limit = request.limit as usize;
        let end = start.saturating_add(limit).min(entries.len());
        let page = entries[start..end].to_vec();
        let next_cursor = (end < entries.len())
            .then(|| page.last().map(|entry| entry.path.clone()))
            .flatten();
        serde_json::to_value(WritingSectionListResult {
            entries: page,
            next_cursor,
        })
        .map_err(|_| errors::internal("Writing section list cannot be encoded"))
    }

    pub(super) fn writing_structure(&self, params: &Value) -> Result<Value> {
        let request: WritingStructureParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid writing structure parameters"))?;
        let file = self.editor.stored(&request.file_id)?.clone();
        require_revision(&request.revision, file.revision)?;
        let record = self.writing_record(&file.file_id)?;
        let text = file.draft_text.as_deref().unwrap_or(&file.disk_text);
        let (slides, mut scenes, elements) = structure(&record.kind, text)?;
        decorate_scenes(&mut scenes, &record.sidecar);
        let result = WritingStructureResult {
            document: public_document(&file, &record)?,
            slides,
            scenes,
            elements,
            variants: record.sidecar.line_variants,
            beats: record.sidecar.beat_cards,
            truncated: false,
        };
        serde_json::to_value(result)
            .map_err(|_| errors::internal("Writing structure cannot be encoded"))
    }
}

fn structure(
    kind: &WritingKind,
    text: &str,
) -> Result<(
    Vec<soyeht_protocol::WritingSlide>,
    Vec<ScreenplayScene>,
    Vec<soyeht_protocol::ScreenplayElement>,
)> {
    match kind {
        WritingKind::Text => Ok((Vec::new(), Vec::new(), Vec::new())),
        WritingKind::Presentation => Ok((
            soyeht_writing::segment_slides(text)?,
            Vec::new(),
            Vec::new(),
        )),
        WritingKind::Screenplay => {
            let screenplay = soyeht_writing::parse_screenplay(text)?;
            Ok((Vec::new(), screenplay.scenes, screenplay.elements))
        }
    }
}

fn decorate_scenes(
    scenes: &mut [ScreenplayScene],
    sidecar: &soyeht_store::writing::WritingSidecar,
) {
    for scene in scenes {
        if let Some(annotation) = sidecar
            .scene_annotations
            .iter()
            .find(|item| item.scene_id == scene.scene_id)
        {
            scene
                .reference_image
                .clone_from(&annotation.reference_image);
            scene.caption.clone_from(&annotation.caption);
            scene.prompt_idea.clone_from(&annotation.prompt_idea);
        }
        scene.linked_beat_ids = sidecar
            .beat_cards
            .iter()
            .filter(|beat| beat.scene_id.as_deref() == Some(scene.scene_id.as_str()))
            .map(|beat| beat.beat_id.clone())
            .collect();
    }
}

fn cursor_start(
    entries: &[soyeht_protocol::WritingSectionEntry],
    cursor: Option<&str>,
) -> Result<usize> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    entries
        .iter()
        .position(|entry| entry.path == cursor)
        .map(|index| index + 1)
        .ok_or_else(|| errors::invalid("Writing section cursor is invalid"))
}

fn title(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .replace(['-', '_'], " ")
}

fn timestamp(value: i64) -> String {
    chrono::DateTime::from_timestamp_millis(value)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
}
