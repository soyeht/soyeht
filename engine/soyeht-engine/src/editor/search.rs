use super::persistence::{next_revision, stored_version};
use crate::{
    core::Core,
    errors::{self, Result},
    model,
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::{
    FileAuthor, FileSearchMatch, FileSearchParams, FileSearchResult, FileSearchScope,
};
use soyeht_store::editor::{
    EditorFile, ProjectReplacement, ProjectReplacementItem, StoredFileVersion, document,
};
use std::path::Path;
use uuid::Uuid;

struct ReplacementPlan {
    journal: ProjectReplacement,
    files: Vec<EditorFile>,
}

impl Core {
    pub fn file_search(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: FileSearchParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid file search parameters"))?;
        if request.replacement.is_some() {
            return self.replace_project(connection, params, request);
        }
        let workspace = model::workspace(&self.store, &request.workspace_id)?;
        let files = self.editor.project_files(Path::new(&workspace.path))?;
        let items = match request.scope.clone().unwrap_or(FileSearchScope::Content) {
            FileSearchScope::Content => self.search_project_content(&request, &files)?,
            FileSearchScope::Path => search_project_paths(&request, &files)?,
        };
        let result = search_result(items, 0)?;
        Ok(self
            .mutate(connection, "file.search", params, |_| {
                Ok((result, Vec::new()))
            })?
            .result)
    }

    fn replace_project(
        &mut self,
        connection: &str,
        params: &Value,
        request: FileSearchParams,
    ) -> Result<Value> {
        if request
            .scope
            .as_ref()
            .is_some_and(|scope| *scope != FileSearchScope::Content)
        {
            return Err(errors::invalid(
                "Replacement is only valid for content search",
            ));
        }
        if let Some(result) = self.repeated_mutation(connection, "file.search", params)? {
            return Ok(result);
        }
        let identity = self.replacement_identity(connection, params, &request)?;
        let journal = match self
            .store
            .file_replacement(&identity.0, &request.idempotency_key)
            .map_err(errors::storage)?
        {
            Some(journal) => {
                if journal.fingerprint != identity.1 {
                    return Err(errors::idempotency_conflict(
                        "Idempotency key belongs to another replacement",
                    ));
                }
                journal
            }
            None => {
                let author = self.file_author(connection)?;
                let plan = self.plan_replacement(&request, identity, author)?;
                if plan.journal.items.is_empty() {
                    return self.finish_empty_replacement(connection, params, plan.journal.matches);
                }
                self.store
                    .begin_file_replacement(&plan.journal, &plan.files)
                    .map_err(errors::storage)?
            }
        };
        self.apply_replacement_items(&journal)?;
        self.finish_replacement(connection, params, &journal)
    }

    fn plan_replacement(
        &self,
        request: &FileSearchParams,
        identity: (String, String),
        author: FileAuthor,
    ) -> Result<ReplacementPlan> {
        let workspace = model::workspace(&self.store, &request.workspace_id)?;
        let project = self.editor.project_files(Path::new(&workspace.path))?;
        let matcher = soyeht_editor::ContentMatcher::new(&request.query, request.case_sensitive)?;
        let replacement = request
            .replacement
            .as_deref()
            .ok_or_else(|| errors::internal("Replacement request lost its replacement"))?;
        let mut files = Vec::new();
        let mut items = Vec::new();
        let mut matches = Vec::new();
        for project_file in project {
            let remaining = search_limit(request).saturating_sub(matches.len());
            if remaining == 0 {
                break;
            }
            self.plan_replacement_file(
                request,
                &matcher,
                replacement,
                &project_file,
                remaining,
                (&mut files, &mut items, &mut matches),
            )?;
        }
        let total_replaced = u32::try_from(matches.len())
            .map_err(|_| errors::internal("Replacement count exceeds the protocol"))?;
        let journal = ProjectReplacement {
            operation_id: Uuid::new_v4().to_string(),
            scope: identity.0,
            idempotency_key: request.idempotency_key.clone(),
            fingerprint: identity.1,
            workspace_id: request.workspace_id.clone(),
            query: request.query.clone(),
            replacement: replacement.into(),
            case_sensitive: request.case_sensitive,
            author,
            matches,
            total_replaced,
            created_ms: self.now_ms(),
            items,
        };
        Ok(ReplacementPlan { journal, files })
    }

    fn plan_replacement_file(
        &self,
        request: &FileSearchParams,
        matcher: &soyeht_editor::ContentMatcher,
        replacement: &str,
        project: &super::ProjectFile,
        remaining: usize,
        outputs: (
            &mut Vec<EditorFile>,
            &mut Vec<ProjectReplacementItem>,
            &mut Vec<FileSearchMatch>,
        ),
    ) -> Result<()> {
        let Some(disk) = self.editor.project_disk_text(project)? else {
            return Ok(());
        };
        let stored = self
            .store
            .editor_file_for_path(&request.workspace_id, &project.relative)
            .map_err(errors::storage)?;
        let dirty_match = if let Some(file) = &stored
            && (file.draft_text.is_some() || file.conflict_text.is_some())
        {
            !matcher
                .search(
                    file.draft_text.as_deref().unwrap_or(&file.disk_text),
                    None,
                    remaining,
                )?
                .matches
                .is_empty()
        } else {
            false
        };
        let found = matcher.search(&disk, Some(replacement), remaining)?;
        let Some(after) = found.replacement else {
            return Err(errors::internal(
                "Replacement search produced no replacement",
            ));
        };
        if found.replaced == 0 || after == disk {
            if dirty_match && found.replaced == 0 {
                return Err(errors::stale_version(
                    "Project replacement includes a dirty file",
                ));
            }
            return Ok(());
        }
        if let Some(file) = &stored {
            let dirty = file.draft_text.is_some() || file.conflict_text.is_some();
            if dirty {
                return Err(errors::stale_version(
                    "Project replacement includes a dirty file",
                ));
            }
            if file.disk_text != disk {
                return Err(errors::stale_version(
                    "Project replacement includes a file changed on disk",
                ));
            }
        }
        let ordinal = u32::try_from(outputs.1.len())
            .map_err(|_| errors::internal("Replacement file count exceeds the protocol"))?;
        let replaced = u32::try_from(found.replaced)
            .map_err(|_| errors::internal("Replacement count exceeds the protocol"))?;
        let file =
            stored.unwrap_or_else(|| detached_file(request, project, disk.clone(), self.now_ms()));
        outputs
            .2
            .extend(found.matches.into_iter().map(|found| FileSearchMatch {
                workspace_id: request.workspace_id.clone(),
                path: project.relative.clone(),
                range: found.range,
                preview: found.preview,
            }));
        outputs.1.push(ProjectReplacementItem {
            ordinal,
            file_id: file.file_id.clone(),
            relative_path: project.relative.clone(),
            before_hash: content_hash(&disk),
            after_hash: content_hash(&after),
            replaced,
            applied: false,
        });
        outputs.0.push(file);
        Ok(())
    }

    fn apply_replacement_items(&mut self, journal: &ProjectReplacement) -> Result<()> {
        let workspace = model::workspace(&self.store, &journal.workspace_id)?;
        let matcher = soyeht_editor::ContentMatcher::new(&journal.query, journal.case_sensitive)?;
        for item in journal.items.iter().filter(|item| !item.applied) {
            self.apply_replacement_item(journal, item, &matcher, Path::new(&workspace.path))?;
        }
        Ok(())
    }

    fn apply_replacement_item(
        &mut self,
        journal: &ProjectReplacement,
        item: &ProjectReplacementItem,
        matcher: &soyeht_editor::ContentMatcher,
        workspace_root: &Path,
    ) -> Result<()> {
        let mut file = self
            .store
            .editor_file(&item.file_id)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::internal("Replacement file disappeared"))?;
        if file.draft_text.is_some() || file.conflict_text.is_some() {
            return Err(errors::stale_version(
                "Project replacement file became dirty",
            ));
        }
        let disk = self.editor.current_disk(workspace_root, &file)?;
        let after = self.replacement_content(journal, item, matcher, &disk)?;
        if content_hash(&disk) == item.before_hash
            && !self
                .editor
                .save_disk(workspace_root, &file, &disk, &after)?
        {
            let current = self.editor.current_disk(workspace_root, &file)?;
            if content_hash(&current) != item.after_hash {
                return Err(errors::stale_version(
                    "Project replacement file changed during write",
                ));
            }
        }
        file.revision = next_revision(file.revision)?;
        file.disk_revision = next_revision(file.disk_revision)?;
        file.disk_text = after.clone();
        file.updated_ms = self.now_ms();
        let version = stored_version(&file, after, journal.author.clone(), self.timestamp());
        self.checkpoint_replacement(journal, item, file, version)
    }

    fn replacement_content(
        &self,
        journal: &ProjectReplacement,
        item: &ProjectReplacementItem,
        matcher: &soyeht_editor::ContentMatcher,
        disk: &str,
    ) -> Result<String> {
        let hash = content_hash(disk);
        if hash == item.after_hash {
            return Ok(disk.into());
        }
        if hash != item.before_hash {
            return Err(errors::stale_version(
                "Project replacement file no longer matches its plan",
            ));
        }
        let found = matcher.search(disk, Some(&journal.replacement), item.replaced as usize)?;
        let after = found
            .replacement
            .ok_or_else(|| errors::internal("Replacement plan produced no content"))?;
        if found.replaced != item.replaced as usize || content_hash(&after) != item.after_hash {
            return Err(errors::stale_version(
                "Project replacement plan no longer matches",
            ));
        }
        Ok(after)
    }

    fn checkpoint_replacement(
        &mut self,
        journal: &ProjectReplacement,
        item: &ProjectReplacementItem,
        file: EditorFile,
        version: StoredFileVersion,
    ) -> Result<()> {
        let open = self.editor.is_open(&file.file_id);
        let events = if open {
            vec![(
                "file.saved".into(),
                json!({"file":document(file.clone()).map_err(errors::storage)?,"by":journal.author}),
            )]
        } else {
            Vec::new()
        };
        let saved = file.clone();
        let key = format!("replace:{}:{}", journal.operation_id, item.ordinal);
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-editor",
            &key,
            &item.after_hash,
            |change| {
                change.mark_file_replacement_applied(
                    &journal.operation_id,
                    item.ordinal,
                    &saved,
                    &version,
                )?;
                Ok((json!({}), events))
            },
        )
        .map_err(errors::storage)?;
        self.editor.replace(file.clone(), self.now_ms());
        self.publish(&mutation.events)?;
        if open {
            self.track_language_file(&file);
        }
        Ok(())
    }

    fn finish_replacement(
        &mut self,
        connection: &str,
        params: &Value,
        journal: &ProjectReplacement,
    ) -> Result<Value> {
        let result = search_result(journal.matches.clone(), journal.total_replaced)?;
        let operation = journal.operation_id.clone();
        Ok(self
            .mutate(connection, "file.search", params, |change| {
                change.complete_file_replacement(&operation)?;
                Ok((result, Vec::new()))
            })?
            .result)
    }

    fn finish_empty_replacement(
        &mut self,
        connection: &str,
        params: &Value,
        matches: Vec<FileSearchMatch>,
    ) -> Result<Value> {
        let result = search_result(matches, 0)?;
        Ok(self
            .mutate(connection, "file.search", params, |_| {
                Ok((result, Vec::new()))
            })?
            .result)
    }

    fn replacement_identity(
        &self,
        connection: &str,
        params: &Value,
        request: &FileSearchParams,
    ) -> Result<(String, String)> {
        let client = self
            .clients
            .get(connection)
            .ok_or_else(|| errors::unauthorized("Connection not found"))?;
        if request.idempotency_key.is_empty() {
            return Err(errors::invalid("Missing idempotency key"));
        }
        let fingerprint = STANDARD.encode(crate::auth::token_hash(
            &json!(["file.search", params]).to_string(),
        ));
        Ok((client.scope.clone(), fingerprint))
    }

    fn search_project_content(
        &self,
        request: &FileSearchParams,
        files: &[super::ProjectFile],
    ) -> Result<Vec<FileSearchMatch>> {
        let mut items = Vec::new();
        let matcher = soyeht_editor::ContentMatcher::new(&request.query, request.case_sensitive)?;
        for file in files {
            if items.len() >= search_limit(request) {
                break;
            }
            let Some(text) = self.editor.project_text(&request.workspace_id, file)? else {
                continue;
            };
            let search = matcher.search(&text, None, search_limit(request) - items.len())?;
            items.extend(search.matches.into_iter().map(|found| FileSearchMatch {
                workspace_id: request.workspace_id.clone(),
                path: file.relative.clone(),
                range: found.range,
                preview: found.preview,
            }));
        }
        Ok(items)
    }
}

fn detached_file(
    request: &FileSearchParams,
    project: &super::ProjectFile,
    disk: String,
    now_ms: i64,
) -> EditorFile {
    EditorFile {
        file_id: Uuid::new_v4().to_string(),
        pane_id: None,
        workspace_id: request.workspace_id.clone(),
        relative_path: project.relative.clone(),
        language: super::service::language(&project.canonical),
        revision: 0,
        disk_revision: 0,
        disk_text: disk,
        draft_text: None,
        draft_author: None,
        conflict_text: None,
        watching: false,
        updated_ms: now_ms,
    }
}

fn search_result(items: Vec<FileSearchMatch>, replaced: u32) -> Result<Value> {
    serde_json::to_value(FileSearchResult { items, replaced })
        .map_err(|_| errors::internal("File search result serialization failed"))
}

fn search_limit(request: &FileSearchParams) -> usize {
    usize::try_from(request.limit)
        .unwrap_or(soyeht_protocol::limits::EDITOR_SEARCH_RESULTS)
        .min(soyeht_protocol::limits::EDITOR_SEARCH_RESULTS)
}

fn content_hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn search_project_paths(
    request: &FileSearchParams,
    files: &[super::ProjectFile],
) -> Result<Vec<FileSearchMatch>> {
    let paths = files
        .iter()
        .map(|file| file.relative.clone())
        .collect::<Vec<_>>();
    let matches = soyeht_editor::rank_paths(&paths, &request.query, request.limit as usize)?;
    Ok(matches
        .into_iter()
        .map(|found| FileSearchMatch {
            workspace_id: request.workspace_id.clone(),
            path: found.path.clone(),
            range: found.range,
            preview: found.path,
        })
        .collect())
}
