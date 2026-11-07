//! Whether the person trusts a project folder (ADR 0150, contract 18).
//!
//! A project folder holds files that some programs run: a language server evaluates its build
//! description, and the repository-defined helpers will read their own instructions from it. So a
//! folder has one answer the person gives, kept by the store per canonical folder, and every
//! feature that would run what the folder contains asks it here instead of keeping its own. A
//! folder the person has not answered is `undecided`, and an undecided folder is restricted.
//!
//! Only the person decides: the signed Mac app (or a paired device, or the local human where the
//! system cannot tell the app from a program), never an agent and never a program that a
//! repository started in a terminal.
use crate::{
    core::Core,
    errors::{self, Result},
    model, ui_attestation,
};
use serde_json::Value;
use soyeht_protocol::{
    ProjectTrust, ProjectTrustChangedDraft, ProjectTrustChoice, ProjectTrustEntry,
    ProjectTrustSetParams, ProjectTrustSetResult, Role, method,
};
use soyeht_store::database::Store;
use std::path::Path;

/// The folder that names `path` in the store: its canonical path, so a link to the folder, or the
/// folder written another way, has the same answer, and a folder replaced by a link to another
/// has not. A path that cannot be resolved names itself.
pub(crate) fn root_of(path: &str) -> String {
    soyeht_platform::fs::canonicalize(Path::new(path)).map_or_else(
        |_| path.to_owned(),
        |root| root.to_string_lossy().into_owned(),
    )
}

fn trust_of(choice: Option<ProjectTrustChoice>) -> ProjectTrust {
    match choice {
        None => ProjectTrust::Undecided,
        Some(ProjectTrustChoice::Restricted) => ProjectTrust::Restricted,
        Some(ProjectTrustChoice::Trusted) => ProjectTrust::Trusted,
    }
}

/// The answer for the folder `path`, read from the store.
pub(crate) fn project_trust(store: &Store, path: &str) -> Result<ProjectTrust> {
    soyeht_store::project_trust::decided(store, &root_of(path))
        .map(trust_of)
        .map_err(errors::storage)
}

/// Whether the files of the folder `path` may run code: only a folder the person trusted.
pub(crate) fn project_trusted(store: &Store, path: &str) -> Result<bool> {
    Ok(project_trust(store, path)? == ProjectTrust::Trusted)
}

impl Core {
    /// The trust of every open workspace, as the `trust` section of a snapshot.
    pub(crate) fn project_trust_snapshot(&self) -> Result<Value> {
        let answers = soyeht_store::project_trust::all(&self.store).map_err(errors::storage)?;
        let workspaces = self.store.workspaces().map_err(errors::storage)?;
        let entries: Vec<ProjectTrustEntry> = workspaces
            .iter()
            .map(|workspace| {
                let root = root_of(&workspace.path);
                ProjectTrustEntry {
                    workspace_id: workspace.workspace_id.clone(),
                    trust: trust_of(
                        answers
                            .iter()
                            .find(|(answered, _)| *answered == root)
                            .map(|(_, choice)| choice.clone()),
                    ),
                }
            })
            .collect();
        serde_json::to_value(entries)
            .map_err(|_| errors::internal("Project trust serialization failed"))
    }

    /// The events a new workspace needs so a client knows its folder was answered before.
    pub(crate) fn project_trust_event_for(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<Vec<(String, Value)>> {
        let trust = project_trust(&self.store, path)?;
        if trust == ProjectTrust::Undecided {
            return Ok(Vec::new());
        }
        Ok(vec![model::pending(&ProjectTrustChangedDraft {
            workspace_ids: vec![workspace_id.to_owned()],
            trust,
        })?])
    }

    /// Whether the connection may decide: the signed Mac app where the system can tell it from a
    /// shell or script, a paired device, and the local human elsewhere (as for pairing).
    fn authorize_project_trust(&self, connection: &str) -> Result<()> {
        let client = self.clients.get(connection);
        let role = client
            .and_then(|client| client.actor.as_ref())
            .map(|actor| &actor.role);
        let app = client.is_some_and(|client| client.signed_ui) || !ui_attestation::available();
        match role {
            Some(Role::HumanDevice) => Ok(()),
            Some(Role::HumanLocal) if app => Ok(()),
            _ => Err(errors::forbidden_role(
                "A project folder is trusted in the Soyeht app, by the person",
            )),
        }
    }

    pub fn project_trust_set(&mut self, connection: &str, params: &Value) -> Result<Value> {
        self.authorize_project_trust(connection)?;
        let request: ProjectTrustSetParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid project trust parameters"))?;
        let workspace = model::workspace(&self.store, &request.workspace_id)?;
        let root = root_of(&workspace.path);
        let workspace_ids: Vec<String> = self
            .store
            .workspaces()
            .map_err(errors::storage)?
            .into_iter()
            .filter(|other| root_of(&other.path) == root)
            .map(|other| other.workspace_id)
            .collect();
        let trust = trust_of(Some(request.trust.clone()));
        let event = model::pending(&ProjectTrustChangedDraft {
            workspace_ids: workspace_ids.clone(),
            trust: trust.clone(),
        })?;
        let result = serde_json::to_value(ProjectTrustSetResult {
            workspace_ids,
            trust,
        })
        .map_err(|_| errors::internal("Project trust result serialization failed"))?;
        let mutation = self.mutate(connection, method::PROJECT_TRUST_SET, params, |tx| {
            soyeht_store::project_trust::decide(tx, &root, &request.trust)?;
            Ok((result, vec![event]))
        })?;
        self.language_servers_follow_trust(&root);
        Ok(mutation.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("sn-trust-{name}-{}", std::process::id()));
        (Store::open(&path).unwrap(), path)
    }

    fn decide(store: &mut Store, root: &str, choice: ProjectTrustChoice) {
        store
            .mutate("test", &format!("trust-{root}"), "trust", |tx| {
                soyeht_store::project_trust::decide(tx, root, &choice)?;
                Ok((Value::Null, Vec::new()))
            })
            .unwrap();
    }

    #[test]
    fn a_folder_nobody_answered_for_is_undecided_and_not_trusted() {
        let (store, path) = store("undecided");
        assert_eq!(
            project_trust(&store, "/no/such/folder").unwrap(),
            ProjectTrust::Undecided
        );
        assert!(!project_trusted(&store, "/no/such/folder").unwrap());
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn only_a_trusted_answer_lets_the_folder_run_its_files() {
        let (mut store, path) = store("answers");
        decide(&mut store, "/no/such/trusted", ProjectTrustChoice::Trusted);
        decide(&mut store, "/no/such/kept", ProjectTrustChoice::Restricted);
        assert!(project_trusted(&store, "/no/such/trusted").unwrap());
        assert!(!project_trusted(&store, "/no/such/kept").unwrap());
        assert_eq!(
            project_trust(&store, "/no/such/kept").unwrap(),
            ProjectTrust::Restricted
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_the_folder_has_the_folders_answer_and_a_swapped_folder_has_none() {
        let (mut store, path) = store("link");
        let folder = path.join("project");
        let other = path.join("other");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let link = path.join("link");
        std::os::unix::fs::symlink(&folder, &link).unwrap();
        decide(
            &mut store,
            &root_of(&folder.to_string_lossy()),
            ProjectTrustChoice::Trusted,
        );
        assert!(project_trusted(&store, &link.to_string_lossy()).unwrap());
        // The same name, now another folder: the answer was about the first.
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&other, &link).unwrap();
        assert!(!project_trusted(&store, &link.to_string_lossy()).unwrap());
        let _ = std::fs::remove_dir_all(path);
    }
}
