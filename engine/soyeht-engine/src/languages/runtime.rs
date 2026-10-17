use super::{DiagnosticUpdate, Server, Service};
use crate::{
    core::Core,
    errors::{self, Result},
    model,
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_lsp::{Position, Range};
use soyeht_platform::log;
use soyeht_protocol::{
    FileDiagnostic, LspStatusParams, LspStatusResult, TextPosition, TextRange, event,
};
use soyeht_ptyd::wire::{Instance, Request, Response};
use soyeht_store::editor::EditorFile;
use std::path::Path;
use uuid::Uuid;

impl Core {
    pub(crate) fn ensure_language_server(&mut self, file: &EditorFile) -> Result<String> {
        let workspace = model::workspace(&self.store, &file.workspace_id)?;
        let key = Service::key(&file.workspace_id, &file.language)?;
        if self.languages.get(&key).is_none() {
            let server = Server::new(&file.workspace_id, &workspace.path, file.language.clone())?;
            self.languages.insert(server)?;
            self.persist_language_server(&key, Vec::new())?;
        }
        if self
            .languages
            .get(&key)
            .and_then(|server| server.instance_id.as_ref())
            .is_none()
        {
            self.spawn_language_server(&key)?;
        }
        self.sync_language_document(&key, file)?;
        Ok(key)
    }

    pub(crate) fn track_language_file(&mut self, file: &EditorFile) {
        if soyeht_lsp::config(&file.language).is_none() {
            return;
        }
        if let Err(error) = self.ensure_language_server(file) {
            if let Ok(key) = Service::key(&file.workspace_id, &file.language) {
                let marked = self.mark_language_unavailable(&key, &error.to_string());
                errors::log_if_failed("engine.lsp_mark_unavailable_failed", marked);
            }
            log::failure("engine.lsp_failed", &error);
        }
    }

    pub fn lsp_status(&mut self, params: &Value) -> Result<Value> {
        let request: LspStatusParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid language server status parameters"))?;
        let files: Vec<_> = self
            .store
            .editor_files()
            .map_err(errors::storage)?
            .into_iter()
            .filter(|file| status_file_matches(file, &request, &self.panes))
            .collect();
        for file in files {
            self.track_language_file(&file);
        }
        let servers = self
            .languages
            .values()
            .filter(|server| {
                request
                    .workspace_id
                    .as_ref()
                    .is_none_or(|value| *value == server.workspace_id)
                    && request
                        .language
                        .as_ref()
                        .is_none_or(|value| *value == server.language)
            })
            .map(Server::status)
            .collect();
        serde_json::to_value(LspStatusResult { servers })
            .map_err(|_| errors::internal("Language server status serialization failed"))
    }

    pub(crate) fn send_language_value(&mut self, key: &str, value: &Value) -> Result<()> {
        let bytes = soyeht_lsp::encode(value).map_err(errors::lsp)?;
        let operation_key = Uuid::new_v4().to_string();
        let encoded = STANDARD.encode(&bytes);
        language_mut(&mut self.languages, key)?
            .pending_writes
            .insert(operation_key.clone(), encoded.clone());
        self.persist_language_server(key, Vec::new())?;
        self.deliver_language_write(key, operation_key, encoded, bytes.len())
    }

    pub(crate) fn poll_language_server(&mut self, key: &str) -> Result<()> {
        let (instance_id, from_seq) = {
            let server = language(&self.languages, key)?;
            (required_instance(server)?, server.next_seq)
        };
        let Response::Output(output) = self.pty_request(Request::Drain {
            instance_id: instance_id.clone(),
            from_seq,
        })?
        else {
            return Err(errors::ptyd_unavailable(
                "Invalid language server output response",
            ));
        };
        if output.gap.is_some() {
            return self.restart_language_server(key, "Language server output was lost");
        }
        let bytes = STANDARD
            .decode(&output.bytes)
            .map_err(|_| errors::ptyd_unavailable("Invalid language server output encoding"))?;
        if output.next_seq == from_seq && bytes.is_empty() && !output.exited {
            return Ok(());
        }
        let was_ready = language(&self.languages, key)?.state == soyeht_protocol::LspState::Ready;
        let updates = language_mut(&mut self.languages, key)?.accept(&bytes)?;
        language_mut(&mut self.languages, key)?.next_seq = output.next_seq;
        let events = diagnostic_events(updates);
        self.persist_language_server(key, events)?;
        self.pty_request(Request::Ack {
            instance_id,
            seq: output.next_seq,
        })?;
        self.publish_language_completions(key)?;
        if output.exited {
            self.restart_language_server(key, "Language server exited")?;
            return Ok(());
        }
        let became_ready =
            !was_ready && language(&self.languages, key)?.state == soyeht_protocol::LspState::Ready;
        if became_ready {
            self.activate_language_server(key)?;
        }
        Ok(())
    }

    pub(crate) fn reconcile_language_servers(&mut self, instances: &[Instance]) -> Result<()> {
        for key in self.languages.keys() {
            let instance_key = language(&self.languages, &key)?.instance_key.clone();
            let existing = instances.iter().find(|instance| {
                instance.spawn.instance_key == instance_key
                    && !instance.closed
                    && instance.exit_code.is_none()
            });
            if !self.language_server_requires_process(&key)? {
                self.park_language_server(&key, existing)?;
                continue;
            }
            if let Some(instance) = existing {
                language_mut(&mut self.languages, &key)?.adopt(instance)?;
                self.persist_language_server(&key, Vec::new())?;
                self.resume_language_writes(&key)?;
                self.publish_language_completions(&key)?;
            } else {
                self.restart_language_server(&key, "Language server process was lost")?;
            }
        }
        Ok(())
    }

    fn language_server_requires_process(&self, key: &str) -> Result<bool> {
        let server = language(&self.languages, key)?;
        if !server.pending_writes.is_empty() || !server.requests.is_empty() {
            return Ok(true);
        }
        for document in server.documents.values() {
            let file = match self.editor.stored(&document.file_id) {
                Ok(file) => file,
                Err(errors::Error::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            if file.pane_id.as_ref().is_some_and(|pane_id| {
                self.panes
                    .get(pane_id)
                    .is_some_and(|pane| !pane.stored.closed)
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn park_language_server(&mut self, key: &str, existing: Option<&Instance>) -> Result<()> {
        if let Some(instance) = existing {
            self.pty_request(Request::Close {
                instance_id: instance.instance_id.clone(),
            })?;
        }
        if existing.is_some() || language(&self.languages, key)?.instance_id.is_some() {
            language_mut(&mut self.languages, key)?
                .restart("No open editor pane requires this language server")?;
            self.persist_language_server(key, Vec::new())?;
        }
        Ok(())
    }

    pub(crate) fn advance_language_servers(&mut self) {
        for key in self.languages.keys() {
            if self
                .languages
                .get(&key)
                .is_none_or(|server| server.instance_id.is_none())
            {
                continue;
            }
            if let Err(error) = self.poll_language_server(&key) {
                let marked = self.mark_language_unavailable(&key, &error.to_string());
                errors::log_if_failed("engine.lsp_mark_unavailable_failed", marked);
                let failed = self.fail_language_waiters(
                    &key,
                    errors::harness_unavailable("Language server became unavailable"),
                );
                // A failure here can leave editor requests waiting: it must reach the log.
                errors::log_if_failed("engine.lsp_fail_waiters_failed", failed);
                log::failure("engine.lsp_failed", &error);
            }
        }
    }

    fn spawn_language_server(&mut self, key: &str) -> Result<()> {
        let spawn = language(&self.languages, key)?.spawn()?;
        let Response::Instance(instance) = self.pty_request(Request::Spawn(spawn))? else {
            return Err(errors::ptyd_unavailable(
                "Invalid language server spawn response",
            ));
        };
        language_mut(&mut self.languages, key)?.adopt(&instance)?;
        self.persist_language_server(key, Vec::new())?;
        let root = root_uri(Path::new(&language(&self.languages, key)?.root))?;
        let initialize = language_mut(&mut self.languages, key)?
            .client
            .initialize(&root)
            .map_err(errors::lsp)?;
        self.send_language_value(key, &initialize)
    }

    fn activate_language_server(&mut self, key: &str) -> Result<()> {
        self.send_language_value(key, &soyeht_lsp::initialized())?;
        let file_ids: Vec<_> = language(&self.languages, key)?
            .documents
            .values()
            .map(|document| document.file_id.clone())
            .collect();
        for file_id in file_ids {
            let file = self.editor.stored(&file_id)?.clone();
            self.sync_language_document(key, &file)?;
        }
        self.start_language_queries(key)?;
        Ok(())
    }

    fn sync_language_document(&mut self, key: &str, file: &EditorFile) -> Result<()> {
        let uri = file_uri(
            Path::new(&language(&self.languages, key)?.root),
            &file.relative_path,
        )?;
        let stale = language_mut(&mut self.languages, key)?
            .retain_current_revision(&file.file_id, file.revision);
        self.fail_language_contexts(
            &stale,
            errors::stale_version("File revision changed during language request"),
        )?;
        let current = language(&self.languages, key)?.documents.get(&uri).cloned();
        let ready = language(&self.languages, key)?.state == soyeht_protocol::LspState::Ready;
        let text = file.draft_text.as_ref().unwrap_or(&file.disk_text);
        let generation = language(&self.languages, key)?.generation;
        if !ready {
            let opened_generation = current.as_ref().map_or(0, |item| item.generation);
            language_mut(&mut self.languages, key)?.documents.insert(
                uri,
                super::state::OpenDocument {
                    file_id: file.file_id.clone(),
                    revision: file.revision,
                    generation: opened_generation,
                },
            );
            self.persist_language_server(key, Vec::new())?;
            return Ok(());
        }
        if current
            .as_ref()
            .is_some_and(|item| item.revision == file.revision && item.generation == generation)
        {
            return Ok(());
        }
        let config = soyeht_lsp::config(&file.language)
            .ok_or_else(|| errors::invalid("File language has no language server"))?;
        let version = i64::try_from(file.revision)
            .map_err(|_| errors::internal("File revision exceeds the language protocol"))?;
        let value = if current
            .as_ref()
            .is_none_or(|item| item.generation != generation)
        {
            soyeht_lsp::did_open(&uri, config.language_id, version, text)
        } else {
            soyeht_lsp::did_change(&uri, version, text)
        };
        self.send_language_document(
            key,
            &uri,
            super::state::OpenDocument {
                file_id: file.file_id.clone(),
                revision: file.revision,
                generation,
            },
            &value,
        )
    }

    fn send_language_document(
        &mut self,
        key: &str,
        uri: &str,
        document: super::state::OpenDocument,
        value: &Value,
    ) -> Result<()> {
        let bytes = soyeht_lsp::encode(value).map_err(errors::lsp)?;
        let operation_key = Uuid::new_v4().to_string();
        let encoded = STANDARD.encode(&bytes);
        let server = language_mut(&mut self.languages, key)?;
        server
            .pending_writes
            .insert(operation_key.clone(), encoded.clone());
        server.documents.insert(uri.into(), document);
        self.persist_language_server(key, Vec::new())?;
        self.deliver_language_write(key, operation_key, encoded, bytes.len())
    }

    pub(crate) fn persist_language_server(
        &mut self,
        key: &str,
        events: Vec<(String, Value)>,
    ) -> Result<()> {
        let checkpoint = self.languages.checkpoint(key)?;
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-lsp",
            &Uuid::new_v4().to_string(),
            "language-server-checkpoint",
            |change| {
                change.save_language_server_checkpoint(&checkpoint)?;
                Ok((json!({}), events))
            },
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)
    }

    fn resume_language_writes(&mut self, key: &str) -> Result<()> {
        let writes: Vec<_> = language(&self.languages, key)?
            .pending_writes
            .iter()
            .map(|(operation, bytes)| (operation.clone(), bytes.clone()))
            .collect();
        for (operation_key, bytes) in writes {
            let decoded = STANDARD
                .decode(&bytes)
                .map_err(|_| errors::internal("Stored language server write is invalid"))?;
            self.deliver_language_write(key, operation_key, bytes, decoded.len())?;
        }
        Ok(())
    }

    fn start_language_queries(&mut self, key: &str) -> Result<()> {
        let contexts = language(&self.languages, key)?.waiting_contexts();
        for context in contexts {
            let query = language(&self.languages, key)?
                .query(&context)
                .cloned()
                .ok_or_else(|| errors::internal("Language request disappeared"))?;
            if let Some(value) = language_mut(&mut self.languages, key)?.queue(&context, query)? {
                self.send_language_value(key, &value)?;
            }
        }
        Ok(())
    }

    fn fail_language_waiters(&mut self, key: &str, error: crate::errors::Error) -> Result<()> {
        let contexts: Vec<_> = language(&self.languages, key)?
            .requests
            .keys()
            .cloned()
            .collect();
        self.fail_language_contexts(&contexts, error)
    }

    fn fail_language_contexts(
        &mut self,
        contexts: &[String],
        error: crate::errors::Error,
    ) -> Result<()> {
        let error = errors::wire(error);
        for context in contexts {
            let waiters = self.language_waiters.remove(context).unwrap_or_default();
            for waiter in waiters {
                if let Some(client) = self.clients.get(&waiter.connection) {
                    client
                        .outbox
                        .send(crate::model::failure(waiter.request_id, error.clone())?);
                }
            }
        }
        Ok(())
    }

    fn deliver_language_write(
        &mut self,
        key: &str,
        operation_key: String,
        bytes: String,
        decoded_len: usize,
    ) -> Result<()> {
        let decoded = STANDARD
            .decode(&bytes)
            .map_err(|_| errors::internal("Stored language server write is invalid"))?;
        let response = self.pty_request(Request::Input {
            instance_id: required_instance(language(&self.languages, key)?)?,
            bytes,
            operation_key: operation_key.clone(),
            fingerprint: STANDARD.encode(Sha256::digest(&decoded)),
        })?;
        if !matches!(response, Response::Accepted { bytes: count } if count == decoded_len) {
            return Err(errors::ptyd_unavailable(
                "Stored language server input was not accepted exactly",
            ));
        }
        language_mut(&mut self.languages, key)?
            .pending_writes
            .remove(&operation_key);
        self.persist_language_server(key, Vec::new())?;
        self.pty_request(Request::ConfirmInput { operation_key })?;
        Ok(())
    }

    fn restart_language_server(&mut self, key: &str, detail: &str) -> Result<()> {
        let pending: Vec<_> = language(&self.languages, key)?
            .pending_writes
            .keys()
            .cloned()
            .collect();
        if let Some(instance_id) = language(&self.languages, key)?.instance_id.clone() {
            // Best effort: the generation bump below makes any late output from it stale.
            let _ = self.pty_request(Request::Close { instance_id });
        }
        language_mut(&mut self.languages, key)?.restart(detail)?;
        self.persist_language_server(key, Vec::new())?;
        for operation_key in pending {
            self.pty_request(Request::ConfirmInput { operation_key })?;
        }
        self.spawn_language_server(key)
    }

    fn mark_language_unavailable(&mut self, key: &str, detail: &str) -> Result<()> {
        let pending: Vec<_> = language(&self.languages, key)?
            .pending_writes
            .keys()
            .cloned()
            .collect();
        if let Some(instance_id) = language(&self.languages, key)?.instance_id.clone() {
            // Best effort: the generation bump below makes any late output from it stale.
            let _ = self.pty_request(Request::Close { instance_id });
        }
        let server = language_mut(&mut self.languages, key)?;
        server.restart(detail)?;
        server.state = soyeht_protocol::LspState::Unavailable;
        server.detail = Some(detail.chars().take(1024).collect());
        self.persist_language_server(key, Vec::new())?;
        for operation_key in pending {
            // Best effort: an unconfirmed receipt is reconciled when the supervisor returns.
            let _ = self.pty_request(Request::ConfirmInput { operation_key });
        }
        Ok(())
    }
}

fn status_file_matches(
    file: &EditorFile,
    request: &LspStatusParams,
    panes: &std::collections::BTreeMap<String, crate::model::RunningPane>,
) -> bool {
    file.pane_id
        .as_ref()
        .is_some_and(|pane_id| panes.get(pane_id).is_some_and(|pane| !pane.stored.closed))
        && request
            .workspace_id
            .as_ref()
            .is_none_or(|value| *value == file.workspace_id)
        && request
            .language
            .as_ref()
            .is_none_or(|value| *value == file.language)
}

pub(super) fn wire_diagnostic(value: soyeht_lsp::Diagnostic) -> FileDiagnostic {
    FileDiagnostic {
        diagnostic_id: Uuid::new_v4().to_string(),
        range: wire_range(value.range),
        severity: value.severity,
        message: value.message,
        source: value.source,
    }
}

pub(crate) fn wire_range(value: Range) -> TextRange {
    TextRange {
        start: wire_position(value.start),
        end: wire_position(value.end),
    }
}

fn wire_position(value: Position) -> TextPosition {
    TextPosition {
        line: value.line,
        column: value.character,
    }
}

fn diagnostic_events(updates: Vec<DiagnosticUpdate>) -> Vec<(String, Value)> {
    updates
        .into_iter()
        .map(|update| {
            (
                event::LSP_DIAGNOSTICS.into(),
                json!({"fileId":update.file_id,"revision":update.revision.to_string(),"items":update.items}),
            )
        })
        .collect()
}

pub(super) fn bounded_diagnostics(
    file_id: &str,
    revision: u64,
    items: Vec<FileDiagnostic>,
) -> Result<(Vec<FileDiagnostic>, usize)> {
    let total = items.len();
    let mut bounded = Vec::new();
    let mut bytes = serde_json::to_vec(&json!({
        "fileId":file_id,
        "revision":revision.to_string(),
        "items":[]
    }))
    .map_err(|_| errors::internal("Language diagnostics serialization failed"))?
    .len();
    for item in items {
        let size = serde_json::to_vec(&item)
            .map_err(|_| errors::internal("Language diagnostics serialization failed"))?
            .len()
            .saturating_add(1);
        if bytes.saturating_add(size) > soyeht_protocol::limits::JSON_BYTES.saturating_sub(1024) {
            break;
        }
        bytes = bytes.saturating_add(size);
        bounded.push(item);
    }
    let omitted = total.saturating_sub(bounded.len());
    Ok((bounded, omitted))
}

fn language<'a>(service: &'a Service, key: &str) -> Result<&'a Server> {
    service
        .get(key)
        .ok_or_else(|| errors::internal("Language server disappeared"))
}

fn language_mut<'a>(service: &'a mut Service, key: &str) -> Result<&'a mut Server> {
    service
        .get_mut(key)
        .ok_or_else(|| errors::internal("Language server disappeared"))
}

fn required_instance(server: &Server) -> Result<String> {
    server
        .instance_id
        .clone()
        .ok_or_else(|| errors::harness_unavailable("Language server is not running"))
}

fn root_uri(root: &Path) -> Result<String> {
    super::uri::path_to_uri(root)
        .ok_or_else(|| errors::invalid("Workspace path must be an absolute UTF-8 path"))
}

pub(crate) fn file_uri(root: &Path, relative: &str) -> Result<String> {
    let path = root.join(relative);
    root_uri(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::{DiagnosticSeverity, TextPosition, TextRange};

    #[test]
    fn diagnostic_publication_is_bounded_after_wire_expansion() {
        let item = FileDiagnostic {
            diagnostic_id: "d".repeat(36),
            range: TextRange {
                start: TextPosition { line: 0, column: 0 },
                end: TextPosition { line: 0, column: 1 },
            },
            severity: DiagnosticSeverity::Warning,
            message: "x".repeat(4096),
            source: "server".into(),
        };
        let (items, omitted) = bounded_diagnostics("file-1", 7, vec![item; 500]).unwrap();
        let event = json!({"fileId":"file-1","revision":"7","items":items});
        assert!(
            serde_json::to_vec(&event).unwrap().len() <= soyeht_protocol::limits::JSON_BYTES - 1024
        );
        assert!(omitted > 0);
    }
}
