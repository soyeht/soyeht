//! Writing document ownership and bounded structural projections.

mod assets;
pub(crate) mod attachments;
mod beats;
mod documents;
mod formatting;
mod structure;
mod variants;

use crate::{core::Core, errors, errors::Result};
use serde_json::Value;
use soyeht_protocol::method;

pub fn handles(method: &str) -> bool {
    matches!(
        method,
        method::WRITING_SECTION_LIST
            | method::WRITING_DOCUMENT_CREATE
            | method::WRITING_DOCUMENT_OPEN
            | method::WRITING_STRUCTURE
            | method::WRITING_FORMAT
            | method::WRITING_ASSET_READ
            | method::WRITING_PREFERENCES_SET
            | method::WRITING_VARIANT_REQUEST
            | method::WRITING_VARIANT_ADD
            | method::WRITING_VARIANT_SELECT
            | method::WRITING_VARIANT_REMOVE
            | method::WRITING_BEAT_LIST
            | method::WRITING_BEAT_PUT
            | method::WRITING_BEAT_DELETE
    )
}

impl Core {
    pub(crate) fn document_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
        writing_image: Option<attachments::WritingImageAttachment>,
    ) -> Result<Value> {
        if crate::editor::handles(method) {
            return self.editor_request(connection, method, params);
        }
        self.writing_request(connection, method, params, writing_image)
    }

    pub(crate) fn writing_snapshot(&self) -> Result<Value> {
        let mut documents = Vec::new();
        for record in self.store.writing_documents().map_err(errors::storage)? {
            let Ok(file) = self.editor.stored(&record.file_id) else {
                continue;
            };
            let live = file.pane_id.as_deref().is_some_and(|pane_id| {
                self.panes.get(pane_id).is_some_and(|pane| {
                    !pane.stored.closed
                        && pane.stored.pane.mode == soyeht_protocol::PaneMode::Writing
                })
            });
            if live {
                documents.push(documents::public_document(file, &record)?);
            }
        }
        Ok(serde_json::json!(documents))
    }

    pub(crate) fn writing_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
        writing_image: Option<attachments::WritingImageAttachment>,
    ) -> Result<Value> {
        match method {
            method::WRITING_DOCUMENT_CREATE => self.writing_document_create(connection, params),
            method::WRITING_DOCUMENT_OPEN => self.writing_document_open(connection, params),
            method::WRITING_STRUCTURE => self.writing_structure(connection, params),
            method::WRITING_FORMAT => self.writing_format(connection, params, writing_image),
            method::WRITING_PREFERENCES_SET => self.writing_preferences(connection, params),
            method::WRITING_VARIANT_REQUEST => self.writing_variant_request(connection, params),
            method::WRITING_VARIANT_ADD => self.writing_variant_add(connection, params),
            method::WRITING_VARIANT_SELECT => self.writing_variant_select(connection, params),
            method::WRITING_VARIANT_REMOVE => self.writing_variant_remove(connection, params),
            method::WRITING_BEAT_LIST => self.writing_beat_list(connection, params),
            method::WRITING_BEAT_PUT => self.writing_beat_put(connection, params),
            method::WRITING_BEAT_DELETE => self.writing_beat_delete(connection, params),
            _ => Err(errors::internal("Unknown writing method")),
        }
    }
}
