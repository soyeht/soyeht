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
use soyeht_ptyd::wire::Instance;

impl Server {
    /// The server as it starts for the trust of its folder.
    pub fn with_trust(mut self, trusted: bool) -> Self {
        self.trusted = trusted;
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

    /// The `initializationOptions` of the server's `initialize`, for the folder's trust.
    pub fn options(&self) -> Result<Option<Value>> {
        soyeht_lsp::initialization_options(
            &self.language,
            self.trusted,
            std::env::var_os("PATH").as_deref(),
        )
        .map_err(|_| {
            errors::harness_unavailable(
                "TypeScript for a restricted folder was not found next to typescript-language-server. Trust the folder to use the one in it.",
            )
        })
    }

    /// Follows the trust of the folder: the next process starts the way the trust asks, or the
    /// server waits, restricted, when it has no safe mode.
    pub fn set_trusted(&mut self, trusted: bool) -> Result<()> {
        self.trusted = trusted;
        self.restart(if trusted {
            "The folder is trusted"
        } else {
            "The folder is restricted"
        })
    }
}

impl Core {
    /// Makes the server `key` start the way the trust of its folder asks (ADR 0150): a server
    /// started for another trust is closed and restarts, and `existing` is the process of it that
    /// the supervisor still holds, if the engine has not adopted it yet. Returns whether the
    /// server changed. The caller starts the new process.
    pub(super) fn follow_language_trust(
        &mut self,
        key: &str,
        existing: Option<&Instance>,
    ) -> Result<bool> {
        let server = language(&self.languages, key)?;
        let trusted = project_trusted(&self.store, &server.root)?;
        if server.trusted == trusted {
            return Ok(false);
        }
        if let Some(instance) = existing {
            self.close_process(&instance.instance_id);
        }
        let pending = self.close_language_process(key)?;
        language_mut(&mut self.languages, key)?.set_trusted(trusted)?;
        self.persist_language_server(key, Vec::new())?;
        self.confirm_language_writes(pending);
        Ok(true)
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
        assert_eq!(server.options().unwrap(), None);

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
    fn a_server_with_a_safe_mode_says_so_while_the_folder_is_restricted() {
        let python = Server::new("workspace-1", "/tmp/workspace-1", EditorLanguage::Python)
            .unwrap()
            .with_trust(false);
        assert_eq!(python.status().safe_mode.is_some(), !cfg!(windows));
        assert_eq!(
            python.spawn().unwrap().argv,
            ["pyright-langserver", "--stdio"]
        );
        assert!(python.spawn().unwrap().env.is_empty());
    }
}
