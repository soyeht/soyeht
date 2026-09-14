use crate::{core::Core, errors, errors::Result};
use serde_json::Value;
use soyeht_protocol::{
    TextEdit, TextPosition, TextRange, WritingFormatParams, WritingFormatResult, WritingKind,
};
use soyeht_store::editor::document;

impl Core {
    pub(super) fn writing_format(
        &mut self,
        connection: &str,
        params: &Value,
        image: Option<super::attachments::WritingImageAttachment>,
    ) -> Result<Value> {
        let request: WritingFormatParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid writing format parameters"))?;
        if let Some(result) = self.repeated_mutation(connection, "writing.format", params)? {
            return Ok(result);
        }
        let current = self.editor.stored(&request.file_id)?.clone();
        super::documents::require_revision(&request.base_revision, current.revision)?;
        let record = self.writing_record(&current.file_id)?;
        if record.kind == WritingKind::Screenplay {
            return Err(errors::writing_kind_refused(
                "Markdown formatting is unavailable for screenplay documents",
            ));
        }
        let source = current.draft_text.as_deref().unwrap_or(&current.disk_text);
        let formatted = soyeht_writing::format_markdown(
            source,
            &request.range,
            request.kind,
            request.value.as_deref(),
        )?;
        let edit = replace_document(source, formatted)?;
        let prepared = self.editor.prepare_edit(
            &request.file_id,
            current.revision,
            &[edit],
            self.file_author(connection)?,
            self.now_ms(),
        )?;
        let result = serde_json::to_value(WritingFormatResult {
            file: document(prepared.file.clone()).map_err(errors::storage)?,
        })
        .map_err(|_| errors::internal("Writing format result cannot be encoded"))?;
        let file = prepared.file.clone();
        let event = super::documents::changed_event(&file, &record)?;
        if let Some(image) = image.as_ref() {
            let workspace = crate::model::workspace(&self.store, &current.workspace_id)?;
            self.editor.create_binary_if_absent_or_matching(
                std::path::Path::new(&workspace.path),
                &image.workspace_relative_path,
                &image.bytes,
            )?;
        }
        let mutation = self.mutate(connection, "writing.format", params, |change| {
            change.save_editor_file(&file)?;
            Ok((result, vec![("writing.changed".into(), event)]))
        })?;
        if !mutation.repeated {
            self.editor.commit_edit(prepared);
            self.track_language_file(&file);
        }
        Ok(mutation.result)
    }
}

fn replace_document(source: &str, text: String) -> Result<TextEdit> {
    let line = u32::try_from(source.bytes().filter(|byte| *byte == b'\n').count())
        .map_err(|_| errors::invalid("Writing document has too many lines"))?;
    let column = u32::try_from(
        source
            .rsplit_once('\n')
            .map_or(source, |(_, tail)| tail)
            .encode_utf16()
            .count(),
    )
    .map_err(|_| errors::invalid("Writing line is too long"))?;
    Ok(TextEdit {
        range: TextRange {
            start: TextPosition { line: 0, column: 0 },
            end: TextPosition { line, column },
        },
        text,
    })
}
