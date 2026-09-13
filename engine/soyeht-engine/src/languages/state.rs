use crate::{errors, errors::Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use soyeht_lsp::{Client, Decoder, Incoming, PositionValue, RequestKind};
use soyeht_protocol::{EditorLanguage, FileDiagnostic, LspServerStatus, LspState};
use soyeht_ptyd::wire::{Instance, ProcessKind, Spawn};
use soyeht_store::languages::LanguageServerCheckpoint;
use std::collections::{BTreeMap, VecDeque};
use uuid::Uuid;

const COMPLETED_LIMIT: usize = 128;
const REQUEST_LIMIT: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenDocument {
    pub file_id: String,
    pub revision: u64,
    pub generation: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Server {
    pub workspace_id: String,
    pub root: String,
    pub language: EditorLanguage,
    pub generation: u32,
    pub instance_key: String,
    pub instance_id: Option<String>,
    pub supervisor_id: Option<String>,
    pub next_seq: u64,
    pub state: LspState,
    pub detail: Option<String>,
    pub client: Client,
    pub decoder: Decoder,
    pub documents: BTreeMap<String, OpenDocument>,
    pub diagnostics: BTreeMap<String, Vec<FileDiagnostic>>,
    pub pending_writes: BTreeMap<String, String>,
    #[serde(default)]
    pub requests: BTreeMap<String, Query>,
    pub completed: BTreeMap<String, Completed>,
    #[serde(default)]
    pub completed_order: VecDeque<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub kind: RequestKind,
    pub file_id: String,
    pub revision: u64,
    pub uri: String,
    pub position: Option<PositionValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Completed {
    pub kind: RequestKind,
    pub result: Value,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DiagnosticUpdate {
    pub file_id: String,
    pub revision: u64,
    pub items: Vec<FileDiagnostic>,
}

#[derive(Default)]
pub struct Service {
    servers: BTreeMap<String, Server>,
}

impl Server {
    pub fn new(workspace_id: &str, root: &str, language: EditorLanguage) -> Result<Self> {
        if soyeht_lsp::config(&language).is_none() {
            return Err(errors::invalid("File language has no language server"));
        }
        Ok(Self {
            workspace_id: workspace_id.into(),
            root: root.into(),
            language,
            generation: 1,
            instance_key: Uuid::new_v4().to_string(),
            instance_id: None,
            supervisor_id: None,
            next_seq: 0,
            state: LspState::Starting,
            detail: None,
            client: Client::default(),
            decoder: Decoder::default(),
            documents: BTreeMap::new(),
            diagnostics: BTreeMap::new(),
            pending_writes: BTreeMap::new(),
            requests: BTreeMap::new(),
            completed: BTreeMap::new(),
            completed_order: VecDeque::new(),
        })
    }

    pub fn spawn(&self) -> Result<Spawn> {
        let config = soyeht_lsp::config(&self.language)
            .ok_or_else(|| errors::invalid("File language has no language server"))?;
        let mut argv = vec![config.command.into()];
        argv.extend(config.args.iter().map(|value| (*value).into()));
        Ok(Spawn {
            instance_key: self.instance_key.clone(),
            generation: self.generation,
            kind: ProcessKind::PipesLsp,
            cwd: self.root.clone(),
            argv,
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
    }

    pub fn adopt(&mut self, instance: &Instance) -> Result<()> {
        if instance.spawn != self.spawn()? {
            return Err(errors::ptyd_unavailable(
                "Language server identity does not match its checkpoint",
            ));
        }
        self.instance_id = Some(instance.instance_id.clone());
        self.supervisor_id = Some(instance.supervisor_id.clone());
        Ok(())
    }

    pub fn restart(&mut self, detail: &str) -> Result<()> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| errors::internal("Language server generation exhausted"))?;
        self.instance_key = Uuid::new_v4().to_string();
        self.instance_id = None;
        self.supervisor_id = None;
        self.next_seq = 0;
        self.state = LspState::Restarting;
        self.detail = Some(detail.into());
        self.client = Client::default();
        self.decoder = Decoder::default();
        self.pending_writes.clear();
        Ok(())
    }

    pub fn status(&self) -> LspServerStatus {
        LspServerStatus {
            language: self.language.clone(),
            state: self.state.clone(),
            generation: self.generation,
            detail: self.detail.clone(),
        }
    }

    pub fn accept(&mut self, bytes: &[u8]) -> Result<Vec<DiagnosticUpdate>> {
        let messages = self.decoder.push(bytes).map_err(errors::lsp)?;
        let mut updates = Vec::new();
        for value in messages {
            match self.client.accept(value).map_err(errors::lsp)? {
                Incoming::Response { call, result } => {
                    if call.kind == RequestKind::Initialize {
                        self.state = LspState::Ready;
                        self.detail = None;
                    } else if self.requests.contains_key(&call.context) {
                        self.save_completed(call.context, call.kind, result, None);
                    }
                }
                Incoming::Failure {
                    call,
                    code,
                    message,
                } => {
                    if self.requests.contains_key(&call.context) {
                        self.save_completed(
                            call.context,
                            call.kind,
                            Value::Null,
                            Some(format!("Language server error {code}: {message}")),
                        );
                    }
                }
                Incoming::Diagnostics { uri, diagnostics } => {
                    if let Some(document) = self.documents.get(&uri) {
                        let items: Vec<FileDiagnostic> = diagnostics
                            .into_iter()
                            .map(super::runtime::wire_diagnostic)
                            .collect();
                        let (items, omitted) = super::runtime::bounded_diagnostics(
                            &document.file_id,
                            document.revision,
                            items,
                        )?;
                        if omitted > 0 {
                            eprintln!(
                                "{}",
                                serde_json::json!({
                                    "event":"engine.lsp_diagnostics_truncated",
                                    "fileId":document.file_id,
                                    "omitted":omitted
                                })
                            );
                        }
                        self.diagnostics
                            .insert(document.file_id.clone(), items.clone());
                        updates.push(DiagnosticUpdate {
                            file_id: document.file_id.clone(),
                            revision: document.revision,
                            items,
                        });
                    }
                }
                Incoming::Notification { .. } => {}
            }
        }
        Ok(updates)
    }

    pub fn queue(&mut self, context: &str, query: Query) -> Result<Option<Value>> {
        if let Some(existing) = self.requests.get(context) {
            if existing != &query {
                return Err(errors::internal("Language request context collided"));
            }
        } else {
            self.make_request_room()?;
            self.requests.insert(context.into(), query);
        }
        if self.completed.contains_key(context)
            || self.state != LspState::Ready
            || self.client.has_context(context)
        {
            return Ok(None);
        }
        let query = self
            .requests
            .get(context)
            .cloned()
            .ok_or_else(|| errors::internal("Language request disappeared"))?;
        let value = match query.kind {
            RequestKind::Symbols => self.client.symbols(&query.uri, context),
            RequestKind::Definition => {
                self.client
                    .definition(&query.uri, required_position(&query)?, context)
            }
            RequestKind::References => {
                self.client
                    .references(&query.uri, required_position(&query)?, context)
            }
            RequestKind::Formatting => self.client.formatting(&query.uri, context),
            RequestKind::Initialize | RequestKind::Shutdown => {
                return Err(errors::internal("Invalid queued language request"));
            }
        }
        .map_err(errors::lsp)?;
        Ok(Some(value))
    }

    pub fn waiting_contexts(&self) -> Vec<String> {
        self.requests
            .keys()
            .filter(|context| {
                !self.completed.contains_key(*context) && !self.client.has_context(context)
            })
            .cloned()
            .collect()
    }

    pub fn retain_current_revision(&mut self, file_id: &str, revision: u64) -> Vec<String> {
        let changed = self
            .documents
            .values()
            .any(|document| document.file_id == file_id && document.revision != revision);
        let stale: Vec<_> = self
            .requests
            .iter()
            .filter(|(_, query)| query.file_id == file_id && query.revision != revision)
            .map(|(context, _)| context.clone())
            .collect();
        self.requests.retain(|context, _| !stale.contains(context));
        self.completed
            .retain(|context, _| self.requests.contains_key(context));
        self.completed_order
            .retain(|context| self.completed.contains_key(context));
        if changed {
            self.diagnostics.remove(file_id);
        }
        stale
    }

    pub fn query(&self, context: &str) -> Option<&Query> {
        self.requests.get(context)
    }

    pub fn completed(&self, context: &str) -> Option<&Completed> {
        self.completed.get(context)
    }

    fn save_completed(
        &mut self,
        context: String,
        kind: RequestKind,
        result: Value,
        error: Option<String>,
    ) {
        if self
            .completed
            .insert(
                context.clone(),
                Completed {
                    kind,
                    result,
                    error,
                },
            )
            .is_none()
        {
            self.completed_order.push_back(context);
        }
        while self.completed_order.len() > COMPLETED_LIMIT {
            if let Some(oldest) = self.completed_order.pop_front() {
                self.completed.remove(&oldest);
                self.requests.remove(&oldest);
            }
        }
    }

    fn make_request_room(&mut self) -> Result<()> {
        while self.requests.len() >= REQUEST_LIMIT {
            let oldest = self
                .completed_order
                .pop_front()
                .ok_or_else(|| errors::pane_busy("Language request limit reached"))?;
            self.completed.remove(&oldest);
            self.requests.remove(&oldest);
        }
        Ok(())
    }

    fn normalize_cache(&mut self) {
        self.completed
            .retain(|context, _| self.requests.contains_key(context));
        self.completed_order
            .retain(|context| self.completed.contains_key(context));
        for context in self.completed.keys() {
            if !self.completed_order.contains(context) {
                self.completed_order.push_back(context.clone());
            }
        }
        while self.completed_order.len() > COMPLETED_LIMIT {
            if let Some(oldest) = self.completed_order.pop_front() {
                self.completed.remove(&oldest);
                self.requests.remove(&oldest);
            }
        }
    }
}

fn required_position(query: &Query) -> Result<PositionValue> {
    query
        .position
        .ok_or_else(|| errors::internal("Language request has no position"))
}

impl Service {
    pub fn restore(checkpoints: Vec<LanguageServerCheckpoint>) -> Result<Self> {
        let mut service = Self::default();
        for checkpoint in checkpoints {
            let mut server: Server = serde_json::from_value(checkpoint.payload)
                .map_err(|_| errors::internal("Stored language server checkpoint is invalid"))?;
            if server.workspace_id != checkpoint.workspace_id
                || server.language != checkpoint.language
                || soyeht_lsp::config(&server.language).is_none()
            {
                return Err(errors::internal(
                    "Stored language server identity is invalid",
                ));
            }
            server.normalize_cache();
            let key = key(&server.workspace_id, &server.language)?;
            if service.servers.insert(key, server).is_some() {
                return Err(errors::internal(
                    "Stored language server identity is duplicated",
                ));
            }
        }
        Ok(service)
    }

    pub fn insert(&mut self, server: Server) -> Result<String> {
        let key = key(&server.workspace_id, &server.language)?;
        if self.servers.insert(key.clone(), server).is_some() {
            return Err(errors::internal("Language server already exists"));
        }
        Ok(key)
    }

    pub fn key(workspace_id: &str, language: &EditorLanguage) -> Result<String> {
        key(workspace_id, language)
    }

    pub fn get(&self, key: &str) -> Option<&Server> {
        self.servers.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Server> {
        self.servers.get_mut(key)
    }

    pub fn values(&self) -> impl Iterator<Item = &Server> {
        self.servers.values()
    }

    pub fn keys(&self) -> Vec<String> {
        self.servers.keys().cloned().collect()
    }

    pub fn has_pending_write(&self, operation_key: &str) -> bool {
        self.servers
            .values()
            .any(|server| server.pending_writes.contains_key(operation_key))
    }

    pub fn checkpoint(&self, key: &str) -> Result<LanguageServerCheckpoint> {
        let server = self
            .get(key)
            .ok_or_else(|| errors::internal("Language server disappeared"))?;
        Ok(LanguageServerCheckpoint {
            workspace_id: server.workspace_id.clone(),
            language: server.language.clone(),
            payload: serde_json::to_value(server)
                .map_err(|_| errors::internal("Language server checkpoint is too large"))?,
        })
    }
}

fn key(workspace_id: &str, language: &EditorLanguage) -> Result<String> {
    let language = serde_json::to_value(language)
        .map_err(|_| errors::internal("Language server identity serialization failed"))?;
    let language = language
        .as_str()
        .ok_or_else(|| errors::internal("Language server identity is invalid"))?;
    Ok(format!("{workspace_id}:{language}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use soyeht_lsp::encode;

    fn rust_server() -> Server {
        Server::new("workspace-1", "/tmp/workspace-1", EditorLanguage::Rust).unwrap()
    }

    fn instance(server: &Server) -> Instance {
        Instance {
            instance_id: "instance-1".into(),
            supervisor_id: "supervisor-1".into(),
            spawn: server.spawn().unwrap(),
            pid: 42,
            closed: false,
            exit_code: None,
        }
    }

    #[test]
    fn spawn_is_fixed_scoped_and_adopted_only_by_exact_identity() {
        let mut server = rust_server();
        let spawn = server.spawn().unwrap();
        assert_eq!(spawn.kind, ProcessKind::PipesLsp);
        assert_eq!(spawn.cwd, "/tmp/workspace-1");
        assert_eq!(spawn.argv, ["rust-analyzer"]);

        let exact = instance(&server);
        server.adopt(&exact).unwrap();
        assert_eq!(server.instance_id.as_deref(), Some("instance-1"));

        let mut wrong = exact;
        wrong.spawn.argv.push("--unexpected".into());
        assert!(server.adopt(&wrong).is_err());
    }

    #[test]
    fn one_checkpoint_per_workspace_language_preserves_restart_state() {
        let mut service = Service::default();
        let mut server = rust_server();
        server.documents.insert(
            "file:///tmp/workspace-1/main.rs".into(),
            OpenDocument {
                file_id: "file-1".into(),
                revision: 7,
                generation: 1,
            },
        );
        server
            .pending_writes
            .insert("write-1".into(), "ZnJhbWU=".into());
        server.completed.insert(
            "symbols:file-1:7".into(),
            Completed {
                kind: RequestKind::Symbols,
                result: json!([{"name":"main"}]),
                error: None,
            },
        );
        server.requests.insert(
            "symbols:file-1:7".into(),
            Query {
                kind: RequestKind::Symbols,
                file_id: "file-1".into(),
                revision: 7,
                uri: "file:///tmp/workspace-1/main.rs".into(),
                position: None,
            },
        );
        let key = service.insert(server.clone()).unwrap();
        assert!(service.insert(server).is_err());

        let checkpoint = service.checkpoint(&key).unwrap();
        let restored = Service::restore(vec![checkpoint]).unwrap();
        let server = restored.get(&key).unwrap();
        assert_eq!(server.documents.len(), 1);
        assert_eq!(server.pending_writes.len(), 1);
        assert_eq!(
            server.completed["symbols:file-1:7"].kind,
            RequestKind::Symbols
        );
        assert_eq!(
            server.completed["symbols:file-1:7"].result[0]["name"],
            "main"
        );
    }

    #[test]
    fn partial_frame_and_request_correlation_survive_a_checkpoint() {
        let mut service = Service::default();
        let mut server = rust_server();
        server.state = LspState::Ready;
        let request = server
            .queue(
                "symbols:file-1:1",
                Query {
                    kind: RequestKind::Symbols,
                    file_id: "file-1".into(),
                    revision: 1,
                    uri: "file:///tmp/workspace-1/main.rs".into(),
                    position: None,
                },
            )
            .unwrap();
        let request = request.unwrap();
        let id = request["id"].as_u64().unwrap();
        let response = encode(&json!({"jsonrpc":"2.0","id":id,"result":[]})).unwrap();
        let split = response.len() - 1;
        assert!(server.accept(&response[..split]).unwrap().is_empty());
        let key = service.insert(server).unwrap();

        let checkpoint = service.checkpoint(&key).unwrap();
        let mut restored = Service::restore(vec![checkpoint]).unwrap();
        assert!(
            restored
                .get_mut(&key)
                .unwrap()
                .accept(&response[split..])
                .unwrap()
                .is_empty()
        );
        assert!(
            restored
                .get(&key)
                .unwrap()
                .completed
                .contains_key("symbols:file-1:1")
        );
    }

    #[test]
    fn restart_changes_process_identity_but_keeps_documents_for_reopen() {
        let mut server = rust_server();
        server.documents.insert(
            "file:///tmp/workspace-1/main.rs".into(),
            OpenDocument {
                file_id: "file-1".into(),
                revision: 3,
                generation: 1,
            },
        );
        server.pending_writes.insert("write".into(), "eA==".into());
        let prior_key = server.instance_key.clone();
        server.restart("lost").unwrap();
        assert_eq!(server.generation, 2);
        assert_ne!(server.instance_key, prior_key);
        assert_eq!(server.documents.len(), 1);
        assert!(server.pending_writes.is_empty());
        assert_eq!(server.state, LspState::Restarting);
    }

    #[test]
    fn queued_queries_survive_restart_and_cached_facts_survive_same_revision_sync() {
        let mut server = rust_server();
        server.state = LspState::Ready;
        server.documents.insert(
            "file:///tmp/workspace-1/main.rs".into(),
            OpenDocument {
                file_id: "file-1".into(),
                revision: 7,
                generation: 1,
            },
        );
        server.diagnostics.insert("file-1".into(), Vec::new());
        let query = Query {
            kind: RequestKind::Symbols,
            file_id: "file-1".into(),
            revision: 7,
            uri: "file:///tmp/workspace-1/main.rs".into(),
            position: None,
        };
        assert!(
            server
                .queue("symbols:file-1:7", query.clone())
                .unwrap()
                .is_some()
        );
        assert!(server.retain_current_revision("file-1", 7).is_empty());
        assert!(server.diagnostics.contains_key("file-1"));

        server.restart("lost").unwrap();
        assert!(server.query("symbols:file-1:7").is_some());
        server.state = LspState::Ready;
        assert!(server.queue("symbols:file-1:7", query).unwrap().is_some());

        assert_eq!(
            server.retain_current_revision("file-1", 8),
            ["symbols:file-1:7"]
        );
        assert!(server.query("symbols:file-1:7").is_none());
        assert!(!server.diagnostics.contains_key("file-1"));
    }

    #[test]
    fn requests_are_bounded_before_the_server_is_ready() {
        let mut server = rust_server();
        for index in 0..REQUEST_LIMIT {
            let context = format!("symbols:file-{index}:1");
            let query = Query {
                kind: RequestKind::Symbols,
                file_id: format!("file-{index}"),
                revision: 1,
                uri: format!("file:///tmp/workspace-1/{index}.rs"),
                position: None,
            };
            assert!(server.queue(&context, query).unwrap().is_none());
        }
        let overflow = Query {
            kind: RequestKind::Symbols,
            file_id: "overflow".into(),
            revision: 1,
            uri: "file:///tmp/workspace-1/overflow.rs".into(),
            position: None,
        };
        assert!(matches!(
            server.queue("symbols:overflow:1", overflow),
            Err(errors::Error::PaneBusy(_))
        ));
    }
}
