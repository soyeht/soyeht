//! Soyeht skills (ADR 0075): the `skills.*` methods over the user's and each
//! workspace's skill folders, change announcements, and the folders an agent
//! session receives. The folder format and layout belong to `soyeht-skills`.

mod delivery;
mod watch;

pub(crate) use delivery::session_directories;
pub use watch::Watches;

use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Skill, SkillScope, SkillsChangedEvent, SkillsCreateParams, SkillsDeleteParams,
    SkillsDeleteResult, SkillsListResult, SkillsReadParams, SkillsReadResult, SkillsUpdateParams,
};
use soyeht_skills::{Entry, Store, home};
use std::path::Path;

const ALLOWED_TOOLS_CHARS: usize = 1024;

pub(crate) fn handles(method: &str) -> bool {
    matches!(
        method,
        "skills.list" | "skills.read" | "skills.create" | "skills.update" | "skills.delete"
    )
}

/// The skill folders of one scope.
pub(crate) struct Scoped {
    store: Store,
    scope: SkillScope,
    workspace_id: Option<String>,
}

impl Scoped {
    fn list(&self) -> Result<Vec<Skill>> {
        let entries = self.store.list().map_err(errors::io)?;
        Ok(entries.into_iter().map(|entry| self.wire(entry)).collect())
    }

    fn wire(&self, entry: Entry) -> Skill {
        let (description, allowed_tools, problem) = match entry.manifest {
            Ok(manifest) => (Some(manifest.description), manifest.allowed_tools, None),
            Err(problem) => (None, None, Some(problem.to_string())),
        };
        Skill {
            name: entry.name,
            scope: self.scope.clone(),
            workspace_id: self.workspace_id.clone(),
            path: entry.directory.display().to_string(),
            description,
            allowed_tools: allowed_tools
                .map(|tools| tools.chars().take(ALLOWED_TOOLS_CHARS).collect()),
            revision: entry.revision,
            problem,
        }
    }

    fn directory(&self) -> String {
        self.store.root().display().to_string()
    }
}

fn skill_error(error: soyeht_skills::Error) -> errors::Error {
    use soyeht_skills::Error;
    let message = error.to_string();
    match error {
        Error::NotFound => errors::not_found(message),
        Error::StaleRevision => errors::stale_version(message),
        Error::Io(error) => errors::io(error),
        Error::InvalidName | Error::AlreadyExists | Error::Invalid(_) => errors::invalid(message),
    }
}

fn parse<T: serde::de::DeserializeOwned>(params: &Value) -> Result<T> {
    serde_json::from_value(params.clone()).map_err(|_| errors::invalid("Invalid skill parameters"))
}

impl Core {
    pub(crate) fn user_skills(&self) -> Scoped {
        Scoped {
            store: Store::new(home::user_store(&self.directory)),
            scope: SkillScope::User,
            workspace_id: None,
        }
    }

    pub(crate) fn workspace_skills(&self, workspace_id: &str) -> Result<Scoped> {
        let workspace = model::workspace(&self.store, workspace_id)?;
        Ok(Scoped {
            store: Store::new(home::workspace_store(Path::new(&workspace.path))),
            scope: SkillScope::Workspace,
            workspace_id: Some(workspace_id.into()),
        })
    }

    fn skills_in(&self, workspace_id: Option<&str>) -> Result<Scoped> {
        match workspace_id {
            Some(id) => self.workspace_skills(id),
            None => Ok(self.user_skills()),
        }
    }

    pub(crate) fn skills_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        if method != "skills.list"
            && method != "skills.read"
            && let Some(result) = self.repeated_mutation(connection, method, params)?
        {
            return Ok(result);
        }
        let result = match method {
            "skills.list" => return self.list_skills(connection, params),
            "skills.read" => {
                let request: SkillsReadParams = parse(params)?;
                let scoped = self.skills_in(request.workspace_id.as_deref())?;
                let (entry, content) = scoped.store.read(&request.name).map_err(skill_error)?;
                return Ok(json!(SkillsReadResult {
                    skill: scoped.wire(entry),
                    content,
                }));
            }
            "skills.create" => {
                let request: SkillsCreateParams = parse(params)?;
                let scoped = self.skills_in(request.workspace_id.as_deref())?;
                let instructions = request.instructions.unwrap_or_default();
                let entry = scoped
                    .store
                    .create(&request.name, &request.description, &instructions)
                    .map_err(skill_error)?;
                json!({"skill":scoped.wire(entry)})
            }
            "skills.update" => {
                let request: SkillsUpdateParams = parse(params)?;
                let scoped = self.skills_in(request.workspace_id.as_deref())?;
                let entry = scoped
                    .store
                    .write(&request.name, &request.content, &request.base_revision)
                    .map_err(skill_error)?;
                json!({"skill":scoped.wire(entry)})
            }
            "skills.delete" => {
                let request: SkillsDeleteParams = parse(params)?;
                let scoped = self.skills_in(request.workspace_id.as_deref())?;
                scoped.store.delete(&request.name).map_err(skill_error)?;
                json!(SkillsDeleteResult {
                    name: request.name,
                    removed: true,
                })
            }
            _ => return Err(errors::invalid("Unknown skills method")),
        };
        let now = self.now_ms();
        self.skill_watches
            .touch(params["workspaceId"].as_str(), now);
        Ok(self
            .mutate(connection, method, params, |_| Ok((result, Vec::new())))?
            .result)
    }

    /// The user's skills, then the workspace's when one is named. Listing a
    /// workspace also follows its folder for this connection.
    fn list_skills(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let user = self.user_skills();
        let mut items = user.list()?;
        let mut workspace_directory = None;
        if let Some(id) = params["workspaceId"].as_str() {
            let workspace = self.workspace_skills(id)?;
            let listed = workspace.list()?;
            self.skill_watches
                .show(connection, id, workspace.store.root(), &listed);
            workspace_directory = Some(workspace.directory());
            items.extend(listed);
        }
        Ok(json!(SkillsListResult {
            items,
            user_directory: user.directory(),
            workspace_directory,
        }))
    }

    /// Prepares the user's skills home and starts following it.
    pub(crate) fn start_skills(&mut self) -> Result<()> {
        home::prepare_user_home(&self.directory).map_err(errors::io)?;
        let user = self.user_skills();
        let listed = user.list()?;
        self.skill_watches.follow_user(user.store.root(), &listed);
        Ok(())
    }

    /// Announces the skill folders that changed since the last call.
    pub(crate) fn advance_skills(&mut self) -> Result<()> {
        let now = self.now_ms();
        let clients = &self.clients;
        let roots = self
            .skill_watches
            .settled(now, |connection| clients.contains_key(connection));
        for (workspace_id, root) in roots {
            let scoped = match &workspace_id {
                Some(id) => Scoped {
                    store: Store::new(root),
                    scope: SkillScope::Workspace,
                    workspace_id: Some(id.clone()),
                },
                None => self.user_skills(),
            };
            let listed = scoped.list()?;
            if self.skill_watches.record(workspace_id.as_deref(), &listed) {
                let event = SkillsChangedEvent {
                    scope: scoped.scope,
                    workspace_id,
                };
                self.transient_event("skills.changed", json!(event))?;
            }
        }
        Ok(())
    }
}
