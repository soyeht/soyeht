//! Writing document ownership and bounded structural projections.

mod documents;
mod formatting;
mod structure;

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
            | "writing.preferences.set"
    )
}

impl Core {
    pub(crate) fn document_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        if crate::editor::handles(method) {
            return self.editor_request(connection, method, params);
        }
        self.writing_request(connection, method, params)
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

    pub fn writing_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        match method {
            "writing.section.list" => self.writing_section_list(params),
            "writing.document.create" => self.writing_document_create(connection, params),
            "writing.document.open" => self.writing_document_open(connection, params),
            "writing.structure" => self.writing_structure(params),
            "writing.format" => self.writing_format(connection, params),
            "writing.preferences.set" => self.writing_preferences(connection, params),
            _ => Err(errors::internal("Unknown writing method")),
        }
    }
}
