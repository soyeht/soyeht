use super::state::Query;
use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
};
use serde::Serialize;
use serde_json::Value;
use soyeht_editor::TextBuffer;
use soyeht_lsp::{PositionValue, RequestKind};
use soyeht_protocol::{
    FileDefinitionResult, FileDiagnosticsParams, FileDiagnosticsResult, FileLocation,
    FilePositionParams, FileReferencesResult, FileSymbol, FileSymbolsParams, FileSymbolsResult,
};
use soyeht_store::editor::EditorFile;
use std::path::Path;

const WAITER_LIMIT: usize = 128;
const RESULT_HEADROOM: usize = 1024;

impl Core {
    pub fn language_request(
        &mut self,
        connection: &str,
        request_id: Value,
        method: &str,
        params: &Value,
    ) -> Result<Outcome> {
        let (file, query, context) = self.prepare_language_query(method, params)?;
        let key = self.ensure_language_server(&file)?;
        if let Some(result) = self.language_result(&key, &context)? {
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

    pub fn file_diagnostics(&mut self, params: &Value) -> Result<Value> {
        let request: FileDiagnosticsParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid file diagnostics parameters"))?;
        let revision = revision(&request.revision)?;
        let file = self.current_language_file(&request.file_id, revision)?;
        let key = self.ensure_language_server(&file)?;
        let items = self
            .languages
            .get(&key)
            .and_then(|server| server.diagnostics.get(&file.file_id))
            .cloned()
            .unwrap_or_default();
        serde_json::to_value(FileDiagnosticsResult { items })
            .map_err(|_| errors::internal("File diagnostics serialization failed"))
    }

    pub(crate) fn publish_language_completions(&mut self, key: &str) -> Result<()> {
        let contexts = self
            .languages
            .get(key)
            .map(|server| server.completed_contexts())
            .unwrap_or_default();
        for context in contexts {
            let query = self
                .languages
                .get(key)
                .and_then(|server| server.query(&context))
                .cloned();
            let formatting = query
                .as_ref()
                .is_some_and(|query| query.kind == RequestKind::Formatting);
            let result = match if formatting {
                self.complete_language_format(key, &context)
            } else {
                self.language_result(key, &context)
            } {
                Ok(Some(result)) => Ok(result),
                Ok(None) => continue,
                Err(error) => Err(errors::wire(error)),
            };
            let waiters = self.language_waiters.remove(&context).unwrap_or_default();
            for (connection, id) in waiters {
                if let Some(client) = self
                    .clients
                    .get(&connection)
                    .filter(|client| client.actor.is_some())
                {
                    let frame = match &result {
                        Ok(value) => crate::model::response(id, value.clone())?,
                        Err(error) => crate::model::failure(id, error.clone())?,
                    };
                    client.outbox.send(frame);
                }
            }
            if formatting
                && result.is_ok()
                && let Some(file_id) = query.map(|query| query.file_id)
            {
                if let Some(server) = self.languages.get_mut(key) {
                    server.remove_request(&context);
                }
                self.persist_language_server(key, Vec::new())?;
                let file = self.editor.stored(&file_id)?.clone();
                self.track_language_file(&file);
            }
        }
        Ok(())
    }

    fn prepare_language_query(
        &self,
        method: &str,
        params: &Value,
    ) -> Result<(EditorFile, Query, String)> {
        let (file_id, revision, position, kind) = match method {
            "file.symbols" => {
                let request: FileSymbolsParams = serde_json::from_value(params.clone())
                    .map_err(|_| errors::invalid("Invalid file symbols parameters"))?;
                (
                    request.file_id,
                    revision(&request.revision)?,
                    None,
                    RequestKind::Symbols,
                )
            }
            "file.definition" | "file.references" => {
                let request: FilePositionParams = serde_json::from_value(params.clone())
                    .map_err(|_| errors::invalid("Invalid file position parameters"))?;
                let kind = if method == "file.definition" {
                    RequestKind::Definition
                } else {
                    RequestKind::References
                };
                (
                    request.file_id,
                    revision(&request.revision)?,
                    Some(request.position),
                    kind,
                )
            }
            _ => return Err(errors::invalid("Language query method unavailable")),
        };
        let file = self.current_language_file(&file_id, revision)?;
        if let Some(position) = &position {
            let text = file.draft_text.as_ref().unwrap_or(&file.disk_text);
            TextBuffer::new(text).byte_at(position)?;
        }
        let workspace = crate::model::workspace(&self.store, &file.workspace_id)?;
        let uri = super::runtime::file_uri(Path::new(&workspace.path), &file.relative_path)?;
        let lsp_position = position.map(|value| PositionValue {
            line: value.line,
            character: value.column,
        });
        let context = query_context(kind, &file.file_id, revision, lsp_position);
        Ok((
            file,
            Query {
                kind,
                file_id,
                revision,
                uri,
                position: lsp_position,
                mutation: None,
            },
            context,
        ))
    }

    pub(super) fn current_language_file(&self, file_id: &str, revision: u64) -> Result<EditorFile> {
        let file = self.editor.stored(file_id)?.clone();
        if file.revision != revision {
            return Err(errors::stale_version("File revision changed"));
        }
        Ok(file)
    }

    pub(super) fn add_language_waiter(
        &mut self,
        context: &str,
        connection: &str,
        request_id: Value,
    ) -> Result<()> {
        let waiters = self.language_waiters.entry(context.into()).or_default();
        if waiters
            .iter()
            .any(|(owner, id)| owner == connection && id == &request_id)
        {
            return Ok(());
        }
        if waiters.len() >= WAITER_LIMIT {
            return Err(errors::pane_busy("Language request waiter limit reached"));
        }
        waiters.push((connection.into(), request_id));
        Ok(())
    }

    pub(super) fn language_result(&self, key: &str, context: &str) -> Result<Option<Value>> {
        let Some(server) = self.languages.get(key) else {
            return Ok(None);
        };
        let Some(query) = server.query(context).cloned() else {
            return Ok(None);
        };
        let Some(completed) = server.completed(context).cloned() else {
            return Ok(None);
        };
        if completed.kind != query.kind {
            return Err(errors::internal("Language response kind changed"));
        }
        if let Some(message) = completed.error {
            return Err(errors::harness_unavailable(message));
        }
        match query.kind {
            RequestKind::Symbols => self.symbol_result(&completed.result).map(Some),
            RequestKind::Definition => self
                .location_result(&query, &completed.result, true)
                .map(Some),
            RequestKind::References => self
                .location_result(&query, &completed.result, false)
                .map(Some),
            RequestKind::Formatting => Ok(completed.public_result),
            _ => Err(errors::internal(
                "Language response cannot serve this method",
            )),
        }
    }

    fn symbol_result(&self, value: &Value) -> Result<Value> {
        let mut items = Vec::new();
        let mut bytes = 128;
        for symbol in soyeht_lsp::symbols(value).map_err(errors::lsp)? {
            let item = FileSymbol {
                name: symbol.name,
                kind: symbol.kind,
                range: super::runtime::wire_range(symbol.range),
                selection_range: super::runtime::wire_range(symbol.selection_range),
            };
            if !push_bounded(&mut items, item, &mut bytes)? {
                break;
            }
        }
        serde_json::to_value(FileSymbolsResult { items })
            .map_err(|_| errors::internal("File symbols serialization failed"))
    }

    fn location_result(&self, query: &Query, value: &Value, definition: bool) -> Result<Value> {
        let file = self.editor.stored(&query.file_id)?;
        let workspace = crate::model::workspace(&self.store, &file.workspace_id)?;
        let mut items = Vec::new();
        let mut bytes = 128;
        for location in soyeht_lsp::locations(value).map_err(errors::lsp)? {
            let Some(item) = self.safe_location(&workspace, location)? else {
                continue;
            };
            if !push_bounded(&mut items, item, &mut bytes)? {
                break;
            }
        }
        if definition {
            serde_json::to_value(FileDefinitionResult {
                location: items.into_iter().next(),
            })
            .map_err(|_| errors::internal("File definition serialization failed"))
        } else {
            serde_json::to_value(FileReferencesResult { items })
                .map_err(|_| errors::internal("File references serialization failed"))
        }
    }

    fn safe_location(
        &self,
        workspace: &soyeht_protocol::Workspace,
        location: soyeht_lsp::Location,
    ) -> Result<Option<FileLocation>> {
        let Some(path) = super::uri::uri_to_path(&location.uri) else {
            return Ok(None);
        };
        let root = Path::new(&workspace.path);
        let Ok(resolved) = self.editor.resolve_absolute(root, &path) else {
            return Ok(None);
        };
        Ok(Some(FileLocation {
            workspace_id: workspace.workspace_id.clone(),
            path: resolved.relative,
            range: super::runtime::wire_range(location.range),
        }))
    }
}

fn revision(value: &str) -> Result<u64> {
    value
        .parse()
        .map_err(|_| errors::invalid("File revision is invalid"))
}

fn query_context(
    kind: RequestKind,
    file_id: &str,
    revision: u64,
    position: Option<PositionValue>,
) -> String {
    let name = match kind {
        RequestKind::Symbols => "symbols",
        RequestKind::Definition => "definition",
        RequestKind::References => "references",
        _ => "unsupported",
    };
    position.map_or_else(
        || format!("{name}:{file_id}:{revision}"),
        |position| {
            format!(
                "{name}:{file_id}:{revision}:{}:{}",
                position.line, position.character
            )
        },
    )
}

fn push_bounded<T: Serialize>(items: &mut Vec<T>, item: T, bytes: &mut usize) -> Result<bool> {
    let size = serde_json::to_vec(&item)
        .map_err(|_| errors::internal("Language result serialization failed"))?
        .len()
        .saturating_add(1);
    if bytes.saturating_add(size)
        > soyeht_protocol::limits::JSON_BYTES.saturating_sub(RESULT_HEADROOM)
    {
        return Ok(false);
    }
    *bytes = bytes.saturating_add(size);
    items.push(item);
    Ok(true)
}
