use crate::{
    core::Core,
    errors::{self, Result},
    model,
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{Workspace, WorkspacePaneLayoutEnvelope};
use std::path::Path;
use uuid::Uuid;

pub fn handles(method: &str) -> bool {
    matches!(
        method,
        "workspace.create"
            | "workspace.layout.set"
            | "workspace.rename"
            | "workspace.close"
            | "workspace.focus"
    )
}

fn workspace_for_action(
    store: &impl StorePort,
    id: &str,
    new_name: Option<&str>,
) -> Result<Workspace> {
    let mut workspace = model::workspace(store, id)?;
    if let Some(name) = new_name {
        workspace.name = name.into();
    }
    Ok(workspace)
}

impl Core {
    pub(crate) fn workspace_after_closing_pane(
        &self,
        workspace_id: &str,
        pane_id: &str,
    ) -> Result<Option<Workspace>> {
        let mut workspace = workspace_for_action(&self.store, workspace_id, None)?;
        let Some(layout) = crate::workspace_layout::removing_pane(&workspace.layout, pane_id)
        else {
            return Ok(None);
        };
        workspace.layout = layout;
        Ok(Some(workspace))
    }

    pub fn workspace_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        match method {
            "workspace.create" => self.workspace_create(connection, params),
            "workspace.layout.set" => self.workspace_layout_set(connection, params),
            "workspace.rename" | "workspace.close" | "workspace.focus" => {
                self.workspace_edit(connection, method, params)
            }
            _ => Err(errors::invalid("Workspace method unavailable")),
        }
    }

    pub fn workspace_create(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let path = errors::param_str(params, "path")?;
        if !Path::new(path).is_absolute() || !Path::new(path).is_dir() {
            return Err(errors::invalid(
                "Workspace path must be an existing absolute directory",
            ));
        }
        let workspace = Workspace {
            workspace_id: Uuid::new_v4().to_string(),
            name: errors::param_str(params, "name")?.into(),
            path: path.into(),
            layout: json!({"panes":[]}),
        };
        let mutation = self.mutate(connection, "workspace.create", params, |tx| {
            tx.insert_workspace(&workspace)?;
            Ok((
                json!({"workspace":workspace}),
                vec![("workspace.created".into(), json!({"workspace":workspace}))],
            ))
        })?;
        Ok(mutation.result)
    }

    pub fn workspace_edit(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let id = errors::param_str(params, "workspaceId")?;
        let new_name = if method == "workspace.rename" {
            Some(errors::param_str(params, "name")?)
        } else {
            None
        };
        let workspace = workspace_for_action(&self.store, id, new_name)?;
        if method == "workspace.rename" {
            return Ok(self
                .mutate(connection, method, params, |tx| {
                    tx.update_workspace(&workspace)?;
                    Ok((
                        json!({"workspace":workspace}),
                        vec![("workspace.renamed".into(), json!({"workspace":workspace}))],
                    ))
                })?
                .result);
        }
        if method == "workspace.focus" {
            let scope = errors::invariant(
                self.clients.get(connection),
                "Workspace connection disappeared",
            )?
            .scope
            .clone();
            return Ok(self
                .mutate(connection, method, params, |tx| {
                    tx.focus_workspace(&scope, id)?;
                    Ok((json!({"workspaceId":id}), vec![]))
                })?
                .result);
        }
        let closed: Vec<_> = self
            .panes
            .values()
            .filter(|p| p.stored.pane.workspace_id == id && !p.stored.closed)
            .map(|p| p.stored.clone())
            .collect();
        let result = self
            .mutate(connection, method, params, |tx| {
                tx.close_workspace(id)?;
                let mut events: Vec<_> = closed
                    .iter()
                    .map(|p| {
                        (
                            "pane.closed".into(),
                            json!({"paneId":p.pane.pane_id,"workspaceId":id}),
                        )
                    })
                    .collect();
                events.push(("workspace.closed".into(), json!({"workspaceId":id})));
                Ok((json!({"workspaceId":id}), events))
            })?
            .result;
        for pane in closed {
            errors::invariant(
                self.panes.get_mut(&pane.pane.pane_id),
                "Workspace pane disappeared after its close committed",
            )?
            .stored
            .closed = true;
            self.finish_close(&pane.pane.pane_id)?;
        }
        Ok(result)
    }

    pub fn workspace_layout_set(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let id = errors::param_str(params, "workspaceId")?;
        let layout: WorkspacePaneLayoutEnvelope = serde_json::from_value(params["layout"].clone())
            .map_err(|_| errors::invalid("Workspace pane layout is invalid"))?;
        crate::workspace_layout::validate(
            id,
            &layout,
            self.panes
                .values()
                .filter(|pane| !pane.stored.closed)
                .map(|pane| pane.stored.pane.clone()),
        )
        .map_err(errors::invalid)?;
        let mut workspace = workspace_for_action(&self.store, id, None)?;
        workspace.layout = serde_json::to_value(layout)
            .map_err(|_| errors::internal("Workspace pane layout serialization failed"))?;
        Ok(self
            .mutate(connection, "workspace.layout.set", params, |tx| {
                tx.update_workspace(&workspace)?;
                Ok((
                    json!({"workspace":workspace}),
                    vec![(
                        "workspace.layout_updated".into(),
                        json!({"workspace":workspace}),
                    )],
                ))
            })?
            .result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::testing::MemoryStore;

    #[test]
    fn workspace_rule_reads_and_renames_through_the_store_port() {
        let mut store = MemoryStore::default();
        store.workspaces.insert(
            "workspace".into(),
            Workspace {
                workspace_id: "workspace".into(),
                name: "Before".into(),
                path: "/project".into(),
                layout: json!({"panes":[]}),
            },
        );

        let renamed = workspace_for_action(&store, "workspace", Some("After")).unwrap();
        assert_eq!(renamed.name, "After");
        assert_eq!(store.workspaces["workspace"].name, "Before");
        assert!(workspace_for_action(&store, "missing", None).is_err());
    }
}
