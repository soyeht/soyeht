//! A language server follows the trust of its folder (ADR 0150, contract 18).
use super::{
    Server,
    runtime::{language, language_mut},
};
use crate::{
    core::Core,
    errors::{self, Result},
    project_trust::project_trusted,
};
use serde_json::Value;
use soyeht_protocol::LspState;
use soyeht_ptyd::wire::{Instance, ProcessKind, Spawn};
use soyeht_store::editor::EditorFile;
use std::{collections::BTreeMap, ffi::OsStr, path::Path};

/// How a server starts: its process, and the options it is initialized with.
pub struct Launch {
    pub spawn: Spawn,
    pub options: Option<Value>,
}

impl Server {
    /// The server as it starts for the trust of its folder.
    pub fn with_trust(mut self, trusted: bool) -> Self {
        self.trusted = trusted;
        self.trust_recorded = true;
        if let Some(reason) = self.refusal() {
            self.state = LspState::Restricted;
            self.detail = Some(reason.into());
        }
        self
    }

    /// Why the server does not start in the folder's trust, if it does not: it has no mode that
    /// keeps the folder's files from running.
    pub fn refusal(&self) -> Option<&'static str> {
        soyeht_lsp::refusal(&self.language, self.trusted)
    }

    /// How the server starts for the trust of its folder, from the engine's `search_path`: the
    /// process and the `initializationOptions` of its `initialize`. Looked at once for a start or
    /// an adoption, since it reads the search path's folders. Without trust the program is named
    /// by its absolute path, the process gets a search path of its own (absolute entries outside
    /// the folder), and a program or a TypeScript of the folder is refused.
    pub fn launch(&self, search_path: Option<&OsStr>) -> Result<Launch> {
        let config = soyeht_lsp::config(&self.language)
            .ok_or_else(|| errors::invalid("File language has no language server"))?;
        let (command, env, options) = if self.trusted {
            (config.command.to_owned(), BTreeMap::new(), None)
        } else {
            if let Some(reason) = self.refusal() {
                return Err(errors::harness_unavailable(reason));
            }
            let search_path = search_path.ok_or_else(|| {
                errors::harness_unavailable("The engine has no search path for language servers")
            })?;
            let restricted =
                soyeht_lsp::restricted_launch(&self.language, search_path, Path::new(&self.root))
                    .map_err(|error| match error {
                        soyeht_lsp::LaunchError::NoProgram => errors::harness_unavailable(
                            "The language server was not found outside the project folder. Trust the folder to use the one in it.",
                        ),
                        soyeht_lsp::LaunchError::NoTrustedTypescript => errors::harness_unavailable(
                            "TypeScript for a restricted folder was not found next to typescript-language-server. Trust the folder to use the one in it.",
                        ),
                    })?;
            (
                restricted.program.to_string_lossy().into_owned(),
                BTreeMap::from([(
                    "PATH".to_owned(),
                    restricted.path.to_string_lossy().into_owned(),
                )]),
                restricted.options,
            )
        };
        let mut argv = vec![command];
        argv.extend(config.args.iter().map(|value| (*value).into()));
        Ok(Launch {
            spawn: Spawn {
                instance_key: self.instance_key.clone(),
                generation: self.generation,
                kind: ProcessKind::PipesLsp,
                cwd: self.root.clone(),
                argv,
                env,
                cols: 80,
                rows: 24,
            },
            options,
        })
    }

    /// The process of a trusted server (it needs no search path).
    #[cfg(test)]
    pub fn spawn(&self) -> Result<Spawn> {
        self.launch(None).map(|launch| launch.spawn)
    }

    /// Follows the trust of the folder: the next process starts the way the trust asks, or the
    /// server waits, restricted, when it has no safe mode.
    pub fn set_trusted(&mut self, trusted: bool) -> Result<()> {
        self.trusted = trusted;
        self.trust_recorded = true;
        self.restart(if trusted {
            "The folder is trusted"
        } else {
            "The folder is restricted"
        })
    }
}

impl Core {
    /// How the server `key` starts, from the engine's search path.
    pub(super) fn language_launch(&self, key: &str) -> Result<Launch> {
        language(&self.languages, key)?.launch(self.language_search_path.as_deref())
    }

    /// Makes the server `key` start the way the trust of its folder asks (ADR 0150): a server
    /// started for another trust, or by an engine that did not record one, is closed and restarts.
    /// `existing` is the process of it that the supervisor still holds, if the engine has not
    /// adopted it yet. Returns whether the server changed. The caller starts the new process.
    pub(super) fn follow_language_trust(
        &mut self,
        key: &str,
        existing: Option<&Instance>,
    ) -> Result<bool> {
        let server = language(&self.languages, key)?;
        let trusted = project_trusted(&self.store, &server.root)?;
        if server.trust_recorded && server.trusted == trusted {
            return Ok(false);
        }
        // The old process is closed with the retrying close, not the best-effort one: a folder
        // that was restricted must not keep a server running because the supervisor was away.
        let held = server.instance_id.clone();
        for instance_id in existing
            .map(|instance| &instance.instance_id)
            .into_iter()
            .chain(&held)
        {
            self.close_process(instance_id);
        }
        let pending: Vec<String> = language(&self.languages, key)?
            .pending_writes
            .keys()
            .cloned()
            .collect();
        self.forget_reads(&crate::read_ahead::language(key));
        language_mut(&mut self.languages, key)?.set_trusted(trusted)?;
        self.persist_language_server(key, Vec::new())?;
        self.confirm_language_writes(pending);
        Ok(true)
    }

    /// Answers an editor request for a restricted server at once, and says why; otherwise the
    /// server `file` needs, started if it is not. The one place the editor's requests go through.
    pub(crate) fn require_language_server(&mut self, file: &EditorFile) -> Result<String> {
        let key = self.ensure_language_server(file)?;
        match self.languages.get(&key).and_then(Server::refusal) {
            Some(reason) => Err(errors::harness_unavailable(reason)),
            None => Ok(key),
        }
    }

    /// A decision of the person about the folder `root`: every language server of that folder
    /// restarts the way the new trust asks, and the ones an open editor needs start again.
    pub(crate) fn language_servers_follow_trust(&mut self, root: &str) {
        for key in self.languages.keys() {
            let in_folder = self
                .languages
                .get(&key)
                .is_some_and(|server| crate::project_trust::root_of(&server.root) == root);
            if !in_folder {
                continue;
            }
            let started = self.follow_language_trust(&key, None).and_then(|changed| {
                let startable = changed
                    && language(&self.languages, &key)?.refusal().is_none()
                    && self.language_server_requires_process(&key)?;
                if startable {
                    self.spawn_language_server(&key)?;
                }
                Ok(())
            });
            if let Err(error) = started {
                self.language_failed(&key, &error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages::{Service, state::Server};
    use soyeht_protocol::EditorLanguage;

    fn rust_server() -> Server {
        Server::new("workspace-1", "/tmp/workspace-1", EditorLanguage::Rust).unwrap()
    }

    #[test]
    fn a_checkpoint_that_does_not_say_it_is_trusted_is_restricted() {
        let mut service = Service::default();
        let key = service.insert(rust_server().with_trust(true)).unwrap();
        let mut checkpoint = service.checkpoint(&key).unwrap();
        assert_eq!(checkpoint.payload["trusted"], true);
        checkpoint
            .payload
            .as_object_mut()
            .unwrap()
            .remove("trusted");
        let restored = Service::restore(vec![checkpoint]).unwrap();
        assert!(!restored.get(&key).unwrap().trusted);
    }

    #[test]
    fn a_server_without_a_safe_mode_waits_restricted_until_the_folder_is_trusted() {
        let server = rust_server().with_trust(false);
        assert_eq!(server.state, LspState::Restricted);
        assert!(
            server
                .detail
                .as_deref()
                .unwrap()
                .contains("Trust the folder")
        );
        assert!(server.status().safe_mode.is_none());

        let mut server = server;
        server.set_trusted(true).unwrap();
        assert_eq!(server.state, LspState::Restarting);
        assert!(server.refusal().is_none());
        assert!(server.status().safe_mode.is_none());
        assert_eq!(server.launch(None).unwrap().options, None);

        // Restricting it again closes it, and a restart keeps it restricted.
        server.set_trusted(false).unwrap();
        assert_eq!(server.state, LspState::Restricted);
        server
            .restart("No open editor pane requires this language server")
            .unwrap();
        assert_eq!(server.state, LspState::Restricted);
        assert_eq!(server.generation, 4);
    }

    #[test]
    fn python_waits_restricted_and_typescript_runs_in_its_safe_mode_while_the_folder_is_restricted()
    {
        let python = Server::new("workspace-1", "/tmp/workspace-1", EditorLanguage::Python)
            .unwrap()
            .with_trust(false);
        assert_eq!(python.state, LspState::Restricted);
        assert!(python.detail.as_deref().unwrap().contains("json.py"));
        assert!(python.status().safe_mode.is_none());
        assert!(python.launch(None).is_err());

        let typescript = Server::new(
            "workspace-1",
            "/tmp/workspace-1",
            EditorLanguage::Typescript,
        )
        .unwrap()
        .with_trust(false);
        assert_eq!(typescript.status().safe_mode, Some(true));
        // It cannot start without a search path to find the installed server on.
        assert!(typescript.launch(None).is_err());

        // Trusted, the supervisor finds the program by name.
        let trusted = Server::new("workspace-1", "/tmp/workspace-1", EditorLanguage::Python)
            .unwrap()
            .with_trust(true);
        assert_eq!(
            trusted.spawn().unwrap().argv,
            ["pyright-langserver", "--stdio"]
        );
        assert!(trusted.spawn().unwrap().env.is_empty());
    }
}
