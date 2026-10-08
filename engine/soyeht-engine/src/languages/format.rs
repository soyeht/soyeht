use super::state::{MutationIntent, Query};
use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_lsp::RequestKind;
use soyeht_protocol::{FileFormatParams, FileFormatResult, TextEdit};
use soyeht_store::editor::document;
use std::path::Path;

impl Core {
    pub fn language_format(
        &mut self,
        connection: &str,
        request_id: Value,
        params: &Value,
    ) -> Result<Outcome> {
        let (file, query, context) = self.prepare_language_format(connection, params)?;
        let key = self.ensure_language_server(&file)?;
        if let Some(result) = self.complete_language_format(&key, &context)? {
            return Ok(Outcome::Reply(result));
        }
        let outbound = self
            .languages
            .get_mut(&key)
            .ok_or_else(|| errors::internal("Language server disappeared"))?
            .queue(&context, query)?;
        if let Some(value) = outbound {
            self.send_language_value(&key, &value)?;
        } else {
            self.persist_language_server(&key, Vec::new())?;
        }
        self.add_language_waiter(&context, connection, request_id)?;
        Ok(Outcome::Queued)
    }

    pub(super) fn complete_language_format(
        &mut self,
        key: &str,
        context: &str,
    ) -> Result<Option<Value>> {
        let (query, completed) = match self.languages.get(key) {
            Some(server) => match (server.query(context), server.completed(context)) {
                (Some(query), Some(completed)) => (query.clone(), completed.clone()),
                _ => return Ok(None),
            },
            None => return Ok(None),
        };
        if query.kind != RequestKind::Formatting || completed.kind != query.kind {
            return Err(errors::internal("Language format identity changed"));
        }
        if let Some(result) = completed.public_result {
            return Ok(Some(result));
        }
        if let Some(message) = completed.error {
            return Err(errors::harness_unavailable(message));
        }
        let intent = query
            .mutation
            .clone()
            .ok_or_else(|| errors::internal("Language format has no durable mutation"))?;
        if let Some(result) = self.completed_format_receipt(key, context, &intent)? {
            return Ok(Some(result));
        }
        self.apply_language_format(key, context, &query, &intent, &completed.result)
            .map(Some)
    }

    fn prepare_language_format(
        &self,
        connection: &str,
        params: &Value,
    ) -> Result<(soyeht_store::editor::EditorFile, Query, String)> {
        let request: FileFormatParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid file format parameters"))?;
        if request.idempotency_key.is_empty() || request.idempotency_key.len() > 128 {
            return Err(errors::invalid("Invalid file format idempotency key"));
        }
        let revision = request
            .base_revision
            .parse()
            .map_err(|_| errors::invalid("File revision is invalid"))?;
        let file = self.current_language_file(&request.file_id, revision)?;
        let workspace = crate::model::workspace(&self.store, &file.workspace_id)?;
        let uri = super::runtime::file_uri(Path::new(&workspace.path), &file.relative_path)?;
        let client = self
            .clients
            .get(connection)
            .ok_or_else(|| errors::unauthorized("Connection not found"))?;
        let fingerprint = STANDARD.encode(crate::auth::token_hash(
            &json!(["file.format", params]).to_string(),
        ));
        let intent = MutationIntent {
            scope: client.scope.clone(),
            key: request.idempotency_key,
            fingerprint,
            author: self.file_author(connection)?,
        };
        let context = format_context(&file.file_id, revision, &intent);
        Ok((
            file,
            Query {
                kind: RequestKind::Formatting,
                file_id: request.file_id,
                revision,
                uri,
                position: None,
                mutation: Some(intent),
            },
            context,
        ))
    }

    fn completed_format_receipt(
        &mut self,
        key: &str,
        context: &str,
        intent: &MutationIntent,
    ) -> Result<Option<Value>> {
        let receipt = StorePort::idempotency_receipt(&self.store, &intent.scope, &intent.key)
            .map_err(errors::storage)?;
        let Some(receipt) = receipt else {
            return Ok(None);
        };
        if receipt.fingerprint != intent.fingerprint {
            return Err(errors::idempotency_conflict(
                "File format key belongs to another mutation",
            ));
        }
        self.languages
            .get_mut(key)
            .ok_or_else(|| errors::internal("Language server disappeared"))?
            .set_public_result(context, Some(receipt.result.clone()))?;
        self.persist_language_server(key, Vec::new())?;
        Ok(Some(receipt.result))
    }

    fn apply_language_format(
        &mut self,
        key: &str,
        context: &str,
        query: &Query,
        intent: &MutationIntent,
        value: &Value,
    ) -> Result<Value> {
        let edits = format_edits(value)?;
        let prepared = self.editor.prepare_edit(
            &query.file_id,
            query.revision,
            &edits,
            intent.author.clone(),
            self.now_ms(),
        )?;
        let result = serde_json::to_value(FileFormatResult {
            file: document(prepared.file.clone()).map_err(errors::storage)?,
        })
        .map_err(|_| errors::internal("File format result serialization failed"))?;
        let file = prepared.file.clone();
        self.languages
            .get_mut(key)
            .ok_or_else(|| errors::internal("Language server disappeared"))?
            .set_public_result(context, Some(result.clone()))?;
        let checkpoint = self.languages.checkpoint(key)?;
        let actor = serde_json::to_value(&intent.author.actor)
            .map_err(|_| errors::internal("File format actor serialization failed"))?;
        let mutation = StorePort::mutate_as(
            &mut self.store,
            &intent.scope,
            &intent.key,
            &intent.fingerprint,
            Some(&actor),
            |change| {
                change.save_editor_file(&file)?;
                change.save_language_server_checkpoint(&checkpoint)?;
                Ok((result.clone(), Vec::new()))
            },
        );
        let mutation = match mutation {
            Ok(mutation) => mutation,
            Err(error) => {
                if let Some(server) = self.languages.get_mut(key) {
                    // The storage error returned below is the failure; this only clears it.
                    let _ = server.set_public_result(context, None);
                }
                return Err(errors::storage(error));
            }
        };
        if !mutation.repeated {
            self.editor.commit_edit(prepared);
        }
        self.publish(&mutation.events)?;
        Ok(mutation.result)
    }
}

fn format_edits(value: &Value) -> Result<Vec<TextEdit>> {
    soyeht_lsp::edits(value)
        .map_err(errors::lsp)?
        .into_iter()
        .map(|edit| {
            Ok(TextEdit {
                range: super::runtime::wire_range(edit.range),
                text: edit.text,
            })
        })
        .collect()
}

fn format_context(file_id: &str, revision: u64, intent: &MutationIntent) -> String {
    let digest = Sha256::digest(
        json!([intent.scope, intent.key, intent.fingerprint])
            .to_string()
            .as_bytes(),
    );
    format!("format:{file_id}:{revision}:{digest:x}")
}
