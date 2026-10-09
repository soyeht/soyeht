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

pub fn handles(method: &str) -> bool {
    matches!(
        method,
        "writing.section.list"
            | "writing.document.create"
            | "writing.document.open"
            | "writing.structure"
            | "writing.format"
            | "writing.asset.read"
            | "writing.preferences.set"
            | "writing.variant.request"
            | "writing.variant.add"
            | "writing.variant.select"
            | "writing.variant.remove"
            | "writing.beat.list"
            | "writing.beat.put"
            | "writing.beat.delete"
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
            "writing.document.create" => self.writing_document_create(connection, params),
            "writing.document.open" => self.writing_document_open(connection, params),
            "writing.structure" => self.writing_structure(connection, params),
            "writing.format" => self.writing_format(connection, params, writing_image),
            "writing.preferences.set" => self.writing_preferences(connection, params),
            "writing.variant.request" => self.writing_variant_request(connection, params),
            "writing.variant.add" => self.writing_variant_add(connection, params),
            "writing.variant.select" => self.writing_variant_select(connection, params),
            "writing.variant.remove" => self.writing_variant_remove(connection, params),
            "writing.beat.list" => self.writing_beat_list(connection, params),
            "writing.beat.put" => self.writing_beat_put(connection, params),
            "writing.beat.delete" => self.writing_beat_delete(connection, params),
            _ => Err(errors::internal("Unknown writing method")),
        }
    }
}
