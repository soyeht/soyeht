use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use serde_json::Value;
use soyeht_protocol::{FileSearchMatch, FileSearchParams, FileSearchResult, FileSearchScope};

impl Core {
    pub fn file_search(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: FileSearchParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid file search parameters"))?;
        if request.replacement.is_some() {
            return Err(errors::invalid(
                "Project replacement requires a write operation",
            ));
        }
        let workspace = model::workspace(&self.store, &request.workspace_id)?;
        let files = self.editor.project_files(workspace.path.as_ref())?;
        let items = match request.scope.clone().unwrap_or(FileSearchScope::Content) {
            FileSearchScope::Content => self.search_project_content(&request, &files)?,
            FileSearchScope::Path => search_project_paths(&request, &files)?,
        };
        let result = serde_json::to_value(FileSearchResult { items, replaced: 0 })
            .map_err(|_| errors::internal("File search result serialization failed"))?;
        Ok(self
            .mutate(connection, "file.search", params, |_| {
                Ok((result, Vec::new()))
            })?
            .result)
    }

    fn search_project_content(
        &self,
        request: &FileSearchParams,
        files: &[super::service::ProjectFile],
    ) -> Result<Vec<FileSearchMatch>> {
        let mut items = Vec::new();
        let limit = usize::try_from(request.limit)
            .unwrap_or(soyeht_protocol::limits::EDITOR_SEARCH_RESULTS)
            .min(soyeht_protocol::limits::EDITOR_SEARCH_RESULTS);
        let matcher = soyeht_editor::ContentMatcher::new(&request.query, request.case_sensitive)?;
        for file in files {
            if items.len() >= limit {
                break;
            }
            let Some(text) = self.editor.project_text(&request.workspace_id, file)? else {
                continue;
            };
            let search = matcher.search(&text, None, limit - items.len())?;
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

fn search_project_paths(
    request: &FileSearchParams,
    files: &[super::service::ProjectFile],
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
