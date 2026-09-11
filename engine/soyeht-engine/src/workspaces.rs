use crate::{
    core::Core,
    errors::{self, Result},
    model,
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{ErrorKind, Workspace};
use std::path::Path;
use uuid::Uuid;

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
    pub fn workspace_create(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let path = params["path"].as_str().expect("validated path");
        if !Path::new(path).is_absolute() || !Path::new(path).is_dir() {
            return Err(errors::error(
                ErrorKind::InvalidParams,
                "Workspace path must be an existing absolute directory",
            ));
        }
        let workspace = Workspace {
            workspace_id: Uuid::new_v4().to_string(),
            name: params["name"].as_str().expect("validated name").into(),
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
        let id = params["workspaceId"].as_str().expect("validated workspace");
        let workspace = workspace_for_action(
            &self.store,
            id,
            (method == "workspace.rename").then(|| params["name"].as_str().expect("name")),
        )?;
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
            let scope = self.clients[connection].scope.clone();
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
            self.panes
                .get_mut(&pane.pane.pane_id)
                .expect("known pane")
                .stored
                .closed = true;
            self.finish_close(&pane.pane.pane_id);
        }
        Ok(result)
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
