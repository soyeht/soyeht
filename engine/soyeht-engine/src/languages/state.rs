use crate::{errors, errors::Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use soyeht_lsp::{Client, Decoder, Incoming, RequestKind};
use soyeht_protocol::{EditorLanguage, FileDiagnostic, LspServerStatus, LspState};
use soyeht_ptyd::wire::{Instance, ProcessKind, Spawn};
use soyeht_store::languages::LanguageServerCheckpoint;
use std::collections::BTreeMap;
use uuid::Uuid;

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
    pub completed: BTreeMap<String, Completed>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Completed {
    pub kind: RequestKind,
    pub result: Value,
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
            completed: BTreeMap::new(),
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
        self.completed.clear();
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
                    } else {
                        self.completed.insert(
                            call.context,
                            Completed {
                                kind: call.kind,
                                result,
                            },
                        );
                    }
                }
                Incoming::Diagnostics { uri, diagnostics } => {
                    if let Some(document) = self.documents.get(&uri) {
                        let items: Vec<FileDiagnostic> = diagnostics
                            .into_iter()
                            .map(super::runtime::wire_diagnostic)
                            .collect();
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
}

impl Service {
    pub fn restore(checkpoints: Vec<LanguageServerCheckpoint>) -> Result<Self> {
        let mut service = Self::default();
        for checkpoint in checkpoints {
            let server: Server = serde_json::from_value(checkpoint.payload)
                .map_err(|_| errors::internal("Stored language server checkpoint is invalid"))?;
            if server.workspace_id != checkpoint.workspace_id
                || server.language != checkpoint.language
                || soyeht_lsp::config(&server.language).is_none()
            {
                return Err(errors::internal(
                    "Stored language server identity is invalid",
                ));
            }
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
        let request = server
            .client
            .symbols("file:///tmp/workspace-1/main.rs", "symbols:file-1:1")
            .unwrap();
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
}
