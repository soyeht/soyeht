use super::{DiagnosticUpdate, Server, Service};
use crate::{
    core::Core,
    errors::{self, Result},
    model,
    ports::StorePort,
    process_start::Started,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_lsp::{Position, Range};
use soyeht_platform::log;
use soyeht_protocol::{
    FileDiagnostic, LspStatusParams, LspStatusResult, TextPosition, TextRange, event,
};
use soyeht_ptyd::wire::{Instance, Request};
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
        self.queue_language_write(key, value, None)
    }

    /// Stores `value` as the server's next pending write (with the document it opens or changes,
    /// if it does) and sends it.
    fn queue_language_write(
        &mut self,
        key: &str,
        value: &Value,
        document: Option<(&str, super::state::OpenDocument)>,
    ) -> Result<()> {
        let encoded = STANDARD.encode(soyeht_lsp::encode(value).map_err(errors::lsp)?);
        let server = language_mut(&mut self.languages, key)?;
        let operation_key = server.next_write_key();
        server
            .pending_writes
            .insert(operation_key.clone(), encoded.clone());
        if let Some((uri, document)) = document {
            server.documents.insert(uri.into(), document);
        }
        self.persist_language_server(key, Vec::new())?;
        self.deliver_language_write(key, operation_key, encoded)
    }

    pub(crate) fn poll_language_server(&mut self, key: &str) -> Result<()> {
        let (instance_id, from_seq) = {
            let server = language(&self.languages, key)?;
            (required_instance(server)?, server.next_seq)
        };
        // The read waits for the supervisor off the lock (ADR 0119): with none ready the server
        // waits for the next tick.
        let owner = crate::read_ahead::language(key);
        let Some(read) = self.take_read(&owner, &instance_id, from_seq, false)? else {
            return Ok(());
        };
        let output = read.output;
        if output.gap.is_some() {
            return self.restart_language_server(key, "Language server output was lost");
        }
        let bytes = STANDARD
            .decode(&output.bytes)
            .map_err(|_| errors::ptyd_unavailable("Invalid language server output encoding"))?;
        // The next read starts while the rest of the poll runs.
        self.start_read(&owner, &instance_id, output.next_seq, false)?;
        if output.next_seq == from_seq && bytes.is_empty() && !output.exited {
            return Ok(());
        }
        let was_ready = language(&self.languages, key)?.state == soyeht_protocol::LspState::Ready;
        let updates = language_mut(&mut self.languages, key)?.accept(&bytes)?;
        language_mut(&mut self.languages, key)?.next_seq = output.next_seq;
        let events = diagnostic_events(updates);
        self.persist_language_server(key, events)?;
        // Nobody waits for it: the supervisor keeps the output a little longer if it fails.
        self.send_later(Request::Ack {
            instance_id,
            seq: output.next_seq,
        });
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
            } else if let Err(error) =
                self.restart_language_server(&key, "Language server process was lost")
            {
                self.leave_language_server_unavailable(&key, error)?;
            }
        }
        Ok(())
    }

    /// A language server the supervisor refuses to start (its project folder is gone) is
    /// marked unavailable. It must not keep the engine from reaching the supervisor.
    fn leave_language_server_unavailable(&mut self, key: &str, error: errors::Error) -> Result<()> {
        if !self.refused_by_supervisor(&error) {
            return Err(error);
        }
        log::failure("engine.lsp_not_restarted", &error);
        self.mark_language_unavailable(key, "Language server could not be restarted")
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
            self.close_process(&instance.instance_id);
        }
        self.forget_reads(&crate::read_ahead::language(key));
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
            // A write whose job ended without a receipt (its link was replaced meanwhile) is still
            // pending: it goes out again, under the same operation key, which the supervisor
            // answers once. A write whose job is on its way is left alone.
            let pending = self
                .languages
                .get(&key)
                .is_some_and(|server| !server.pending_writes.is_empty());
            let sent = if pending {
                self.resume_language_writes(&key)
            } else {
                Ok(())
            };
            if let Err(error) = sent.and_then(|()| self.poll_language_server(&key)) {
                self.language_failed(&key, &error);
            }
        }
    }

    /// Asks the supervisor for the server's process, off the lock (ADR 0126). The server is
    /// adopted when it has made it; a server restarted meanwhile starts under its new identity.
    fn spawn_language_server(&mut self, key: &str) -> Result<()> {
        let spawn = language(&self.languages, key)?.spawn()?;
        let (owner, key, identity) = (
            format!("lsp:{key}:{}", spawn.instance_key),
            key.to_owned(),
            spawn.instance_key.clone(),
        );
        self.start_process(&owner, spawn, &[], move |core, started| {
            core.finish_language_start(&key, &identity, started);
            Ok(())
        })
    }

    /// Takes the answer of the start of a language server's process under the lock. The process
    /// of a server that was restarted or parked meanwhile is closed; a failure leaves the server
    /// unavailable, as a failed poll does.
    fn finish_language_start(&mut self, key: &str, identity: &str, started: Started) {
        let result = match started {
            Started::Process(instance) => {
                if !self.language_waits_for(key, identity) {
                    // Unless the reconciliation of a new link already adopted this very process.
                    let adopted = self.languages.get(key).is_some_and(|server| {
                        server.instance_id.as_deref() == Some(instance.instance_id.as_str())
                    });
                    if !adopted {
                        self.close_process(&instance.instance_id);
                    }
                    return;
                }
                self.adopt_language_process(key, &instance)
            }
            // A failure of a server restarted meanwhile is not the new server's.
            Started::Failed(error) if self.language_waits_for(key, identity) => Err(error),
            Started::Failed(_) => return,
            // The reconciliation of the link that replaced it adopts or restarts every server.
            Started::Stale => return,
        };
        if let Err(error) = result {
            self.language_failed(key, &error);
        }
    }

    /// Whether the server `key` is still waiting for the process started under `identity`.
    fn language_waits_for(&self, key: &str, identity: &str) -> bool {
        self.languages
            .get(key)
            .is_some_and(|server| server.instance_key == identity && server.instance_id.is_none())
    }

    /// Leaves the server unavailable and fails the editor requests waiting for it, after a failure
    /// that stops it.
    fn language_failed(&mut self, key: &str, error: &errors::Error) {
        let marked = self.mark_language_unavailable(key, &error.to_string());
        errors::log_if_failed("engine.lsp_mark_unavailable_failed", marked);
        let failed = self.fail_language_waiters(
            key,
            errors::harness_unavailable("Language server became unavailable"),
        );
        // A failure here can leave editor requests waiting: it must reach the log.
        errors::log_if_failed("engine.lsp_fail_waiters_failed", failed);
        log::failure("engine.lsp_failed", error);
    }

    fn adopt_language_process(&mut self, key: &str, instance: &Instance) -> Result<()> {
        language_mut(&mut self.languages, key)?.adopt(instance)?;
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
        self.queue_language_write(key, value, Some((uri, document)))
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
            self.deliver_language_write(key, operation_key, bytes)?;
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

    /// Sends a write of the server's pending writes to its process, off the lock (ADR 0126). It
    /// stays pending, and is stored so, until the supervisor has accepted all of it. A write
    /// already on its way is left alone.
    fn deliver_language_write(
        &mut self,
        key: &str,
        operation_key: String,
        bytes: String,
    ) -> Result<()> {
        // The tick asks for every pending write: one on its way costs nothing here.
        let job = format!("lsp-write:{key}:{operation_key}");
        if !self.background.accepts(&job) {
            return Ok(());
        }
        let decoded = STANDARD
            .decode(&bytes)
            .map_err(|_| errors::internal("Stored language server write is invalid"))?;
        let decoded_len = decoded.len();
        let instance_id = required_instance(language(&self.languages, key)?)?;
        let request = Request::Input {
            instance_id: instance_id.clone(),
            bytes,
            operation_key: operation_key.clone(),
            fingerprint: STANDARD.encode(Sha256::digest(&decoded)),
        };
        let key = key.to_owned();
        self.start_input(job, &instance_id, request, move |core, accepted| {
            core.settle_language_write(&key, operation_key, decoded_len, accepted);
            Ok(())
        })
    }

    /// Takes the receipt of a write under the lock. A server restarted meanwhile no longer holds
    /// the write, and its process is not the one the answer is about: the answer, a failure
    /// included, says nothing about the new one, and a receipt is only confirmed. A failure of a
    /// write the server still holds leaves it unavailable.
    fn settle_language_write(
        &mut self,
        key: &str,
        operation_key: String,
        decoded_len: usize,
        accepted: Result<Option<usize>>,
    ) {
        let held = self
            .languages
            .get(key)
            .is_some_and(|server| server.pending_writes.contains_key(&operation_key));
        let confirm = if held {
            match self.finish_language_write(key, &operation_key, decoded_len, accepted) {
                Ok(confirm) => confirm,
                Err(error) => return self.language_failed(key, &error),
            }
        } else {
            matches!(accepted, Ok(Some(_)))
        };
        if confirm {
            // Nobody waits for it: an unconfirmed receipt is reconciled when the supervisor
            // returns.
            self.send_later(Request::ConfirmInput { operation_key });
        }
    }

    /// Makes the receipt of a write the server holds durable. `false` for the failure of a link
    /// that was replaced meanwhile: the write stays pending.
    fn finish_language_write(
        &mut self,
        key: &str,
        operation_key: &str,
        decoded_len: usize,
        accepted: Result<Option<usize>>,
    ) -> Result<bool> {
        let Some(accepted) = accepted? else {
            return Ok(false);
        };
        if accepted != decoded_len {
            return Err(errors::ptyd_unavailable(
                "Stored language server input was not accepted exactly",
            ));
        }
        language_mut(&mut self.languages, key)?
            .pending_writes
            .remove(operation_key);
        self.persist_language_server(key, Vec::new())?;
        Ok(true)
    }

    fn restart_language_server(&mut self, key: &str, detail: &str) -> Result<()> {
        let pending = self.close_language_process(key)?;
        language_mut(&mut self.languages, key)?.restart(detail)?;
        self.persist_language_server(key, Vec::new())?;
        self.confirm_language_writes(pending);
        self.spawn_language_server(key)
    }

    /// Sends the close of the server's process, and returns the writes it never confirmed. Best
    /// effort: the generation bump that follows makes any late output from it stale, and a
    /// replacement starts under a new instance key.
    fn close_language_process(&mut self, key: &str) -> Result<Vec<String>> {
        let pending = language(&self.languages, key)?
            .pending_writes
            .keys()
            .cloned()
            .collect();
        if let Some(instance_id) = language(&self.languages, key)?.instance_id.clone() {
            self.send_later(Request::Close { instance_id });
        }
        self.forget_reads(&crate::read_ahead::language(key));
        Ok(pending)
    }

    /// Confirms the receipts of writes that will not be delivered. Nobody waits for it: an
    /// unconfirmed receipt is reconciled when the supervisor returns.
    fn confirm_language_writes(&mut self, operation_keys: Vec<String>) {
        for operation_key in operation_keys {
            self.send_later(Request::ConfirmInput { operation_key });
        }
    }

    fn mark_language_unavailable(&mut self, key: &str, detail: &str) -> Result<()> {
        let pending = self.close_language_process(key)?;
        let server = language_mut(&mut self.languages, key)?;
        server.restart(detail)?;
        server.state = soyeht_protocol::LspState::Unavailable;
        server.detail = Some(detail.chars().take(1024).collect());
        self.persist_language_server(key, Vec::new())?;
        self.confirm_language_writes(pending);
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
mod tests;
