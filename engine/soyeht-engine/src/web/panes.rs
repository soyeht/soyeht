use crate::{
    core::{Core, persist_pane},
    errors::{self, Result},
    model::{self, RunningPane, StoredPane},
    panes::{pane_created, persist_new_pane},
    ports::StorePort,
    screen::Screen,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Pane, PaneCreateParams, PaneMode, Role, WebCloseParams, WebOpenResult, WebSessionState,
    WebViewportPreset, limits, method,
};
use soyeht_web::{DEFAULT_ZOOM_PERCENT, NormalizedUrl, viewport};
use uuid::Uuid;

const DEFAULT_VIEWPORT_WIDTH: u32 = 1_280;
const DEFAULT_VIEWPORT_HEIGHT: u32 = 720;

pub(crate) struct NewWebPane<'a> {
    pub workspace: &'a soyeht_protocol::Workspace,
    pub name: Option<&'a str>,
    pub url: Option<&'a NormalizedUrl>,
    pub incognito: bool,
}

impl Core {
    pub(crate) fn web_close(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: WebCloseParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid web close parameters"))?;
        let actor = self
            .clients
            .get(connection)
            .and_then(|client| client.actor.clone())
            .ok_or_else(|| errors::unauthorized("Web close connection is unavailable"))?;
        let pane = self
            .panes
            .get(&request.pane_id)
            .filter(|pane| !pane.stored.closed && pane.stored.pane.mode == PaneMode::Web)
            .ok_or_else(|| errors::not_found("Web pane not found"))?;
        self.authorize_web_workspace(&actor, &pane.stored.pane.workspace_id)?;
        if !matches!(
            actor.role,
            Role::Agent | Role::Chief | Role::HumanLocal | Role::HumanDevice
        ) {
            return Err(errors::forbidden_role(
                "Web panes may be closed only by a person or a bound agent",
            ));
        }
        self.pane_edit(connection, method::PANE_CLOSE, params)
    }

    pub(crate) fn web_pane_create(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: PaneCreateParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid web pane parameters"))?;
        let workspace = model::workspace(&self.store, &request.workspace_id)?;
        let url = request
            .url
            .as_deref()
            .map(NormalizedUrl::parse)
            .transpose()?;
        self.persist_web_pane(
            connection,
            method::PANE_CREATE,
            params,
            NewWebPane {
                workspace: &workspace,
                name: request.name.as_deref(),
                url: url.as_ref(),
                incognito: request.incognito.unwrap_or(false),
            },
        )
    }

    pub(crate) fn persist_web_pane(
        &mut self,
        connection: &str,
        method: &str,
        raw_params: &Value,
        intent: NewWebPane<'_>,
    ) -> Result<Value> {
        self.ensure_web_pane_capacity(&intent.workspace.workspace_id)?;
        let pane = web_pane(intent.workspace, intent.name, intent.url, intent.incognito)?;
        let saved = StoredPane {
            pane: pane.clone(),
            closed: false,
            instance_key: Uuid::new_v4().to_string(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        };
        let screen = Screen::open(
            &self.directory.join("sessions").join(&saved.pane.pane_id),
            false,
        )?;
        let (_, events) = pane_created(&saved)?;
        let mutation = self.mutate(connection, method, raw_params, |change| {
            persist_new_pane(change, &saved, None)?;
            let result = if method == method::WEB_OPEN {
                serde_json::to_value(WebOpenResult {
                    pane_id: saved.pane.pane_id.clone(),
                })?
            } else {
                json!({"paneId":saved.pane.pane_id})
            };
            Ok((result, events))
        })?;
        if !mutation.repeated {
            self.panes.insert(
                pane.pane_id.clone(),
                RunningPane {
                    stored: saved,
                    screen,
                },
            );
        }
        Ok(mutation.result)
    }

    /// Closes a web pane that never got a page. A pane that has an address, because the host
    /// reported one, is left to the person.
    pub(super) fn abandon_web_pane(&mut self, pane_id: &str) -> Result<()> {
        let Some(running) = self.panes.get(pane_id).filter(|running| {
            !running.stored.closed
                && running.stored.pane.mode == PaneMode::Web
                && running.stored.pane.url.is_none()
        }) else {
            return Ok(());
        };
        let mut events = Vec::new();
        let mut saved = running.stored.clone();
        let layout = self.prepare_pane_close(&mut saved, &mut events)?;
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine",
            &format!("web-open-abandon:{pane_id}"),
            "web.open.abandon",
            |change| {
                persist_pane(change, &saved)?;
                if let Some(workspace) = &layout {
                    change.update_workspace(workspace)?;
                }
                Ok((json!({}), events))
            },
        )
        .map_err(errors::storage)?;
        errors::invariant(self.panes.get_mut(pane_id), "Web pane disappeared")?.stored = saved;
        self.finish_close(pane_id)?;
        self.publish(&mutation.events)
    }

    fn ensure_web_pane_capacity(&self, workspace_id: &str) -> Result<()> {
        let count = self
            .panes
            .values()
            .filter(|running| {
                !running.stored.closed
                    && running.stored.pane.workspace_id == workspace_id
                    && running.stored.pane.mode == PaneMode::Web
            })
            .count();
        if count >= limits::WEB_PANES_PER_WORKSPACE {
            Err(errors::invalid("Workspace web pane limit reached"))
        } else {
            Ok(())
        }
    }
}

fn web_pane(
    workspace: &soyeht_protocol::Workspace,
    name: Option<&str>,
    url: Option<&NormalizedUrl>,
    incognito: bool,
) -> Result<Pane> {
    Ok(Pane {
        url: url.map(|value| value.as_str().to_owned()),
        viewport: Some(viewport(
            WebViewportPreset::Responsive,
            Some(DEFAULT_VIEWPORT_WIDTH),
            Some(DEFAULT_VIEWPORT_HEIGHT),
            false,
            DEFAULT_ZOOM_PERCENT,
        )?),
        incognito: Some(incognito),
        web_session: Some(if incognito {
            WebSessionState::Incognito
        } else {
            WebSessionState::Anonymous
        }),
        ..model::new_pane(
            PaneMode::Web,
            workspace.workspace_id.clone(),
            name.unwrap_or("Web"),
            workspace.path.clone(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn web_pane_defaults_are_process_free_and_normalized() {
        let root = std::env::temp_dir().join(format!("soyeht-web-pane-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let workspace = soyeht_protocol::Workspace {
            workspace_id: "workspace".into(),
            name: "Workspace".into(),
            path: root.to_string_lossy().into_owned(),
            layout: json!({}),
        };
        let request: PaneCreateParams = serde_json::from_value(json!({
            "workspaceId":"workspace", "mode":"web", "url":"localhost:5173",
            "incognito":true, "idempotencyKey":"create-web"
        }))
        .unwrap();
        let url = NormalizedUrl::parse(request.url.as_deref().unwrap()).unwrap();
        let pane = web_pane(
            &workspace,
            request.name.as_deref(),
            Some(&url),
            request.incognito.unwrap_or(false),
        )
        .unwrap();
        assert_eq!(pane.url.as_deref(), Some("http://localhost:5173/"));
        assert_eq!(pane.web_session, Some(WebSessionState::Incognito));
        assert_eq!(pane.viewport.unwrap().width, DEFAULT_VIEWPORT_WIDTH);
        assert!(pane.pid.is_none() && pane.stream_id.is_none() && !pane.record);
        fs::remove_dir_all(root).unwrap();
    }
}
