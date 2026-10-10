use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    model,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    PaneMode, WebCommandKind, WebCommandParams, WebInteractionMode, WebNavigateParams,
    WebOpenParams, WebOpenResult,
};
use soyeht_web::NormalizedUrl;

use super::panes::NewWebPane;

const NAVIGATION_TIMEOUT_MS: u32 = 30_000;

impl Core {
    pub(crate) fn web_open(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: WebOpenParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid web open parameters"))?;
        if request.mode == Some(WebInteractionMode::Human) {
            return Err(soyeht_web::Error::HumanModeUnavailable.into());
        }
        let url = NormalizedUrl::parse(&request.url)?;
        let (scope, workspace_id) = self.web_open_context(connection, request.workspace_id)?;
        let workspace = model::workspace(&self.store, &workspace_id)?;
        if !request.new_pane.unwrap_or(false)
            && let Some(pane_id) = self.matching_web_pane(
                &workspace_id,
                url.as_str(),
                request.incognito.unwrap_or(false),
            )
        {
            return self.focus_open_web_pane(connection, params, &scope, &workspace_id, &pane_id);
        }
        self.persist_web_pane(
            connection,
            "web.open",
            params,
            NewWebPane {
                workspace: &workspace,
                name: None,
                url: Some(&url),
                incognito: request.incognito.unwrap_or(false),
            },
        )
    }

    pub(crate) fn web_navigate(
        &mut self,
        connection: &str,
        request_id: Value,
        params: &Value,
    ) -> Result<Outcome> {
        let request: WebNavigateParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid web navigation parameters"))?;
        let args = match (request.url.as_deref(), request.action.as_ref()) {
            (Some(url), None) => json!({"url":NormalizedUrl::parse(url)?.as_str()}),
            (None, Some(action)) => json!({"action":action}),
            _ => {
                return Err(errors::invalid(
                    "Web navigation requires exactly one URL or action",
                ));
            }
        };
        let command = WebCommandParams {
            pane_id: request.pane_id,
            command: WebCommandKind::Navigate,
            args,
            timeout_ms: NAVIGATION_TIMEOUT_MS,
            operation_key: request.idempotency_key,
        };
        self.queue_web_command(connection, request_id, "web.navigate", params, command)?;
        Ok(Outcome::Queued)
    }

    fn web_open_context(
        &self,
        connection: &str,
        requested_workspace: Option<String>,
    ) -> Result<(String, String)> {
        let client = self
            .clients
            .get(connection)
            .filter(|client| client.actor.is_some() && !client.outbox.is_closed())
            .ok_or_else(|| errors::unauthorized("Web open connection is unavailable"))?;
        let actor = client
            .actor
            .as_ref()
            .ok_or_else(|| errors::unauthorized("Web open connection is unavailable"))?;
        let scope = client.scope.clone();
        if let Some(bound) = self.bound_web_workspace(actor)? {
            if requested_workspace
                .as_deref()
                .is_some_and(|requested| requested != bound)
            {
                return Err(errors::forbidden_role(
                    "Bound agents may open web panes only in their workspace",
                ));
            }
            return Ok((scope, bound));
        }
        let workspace_id = if let Some(id) = requested_workspace {
            id
        } else {
            self.store
                .client_focus(&client.scope)
                .map_err(errors::storage)?
                .and_then(|focus| focus.workspace_id)
                .ok_or_else(|| errors::invalid("Web open requires a focused workspace"))?
        };
        Ok((scope, workspace_id))
    }

    fn matching_web_pane(&self, workspace_id: &str, url: &str, incognito: bool) -> Option<String> {
        self.panes
            .values()
            .find(|running| {
                let pane = &running.stored.pane;
                !running.stored.closed
                    && pane.workspace_id == workspace_id
                    && pane.mode == PaneMode::Web
                    && pane.url.as_deref() == Some(url)
                    && pane.incognito == Some(incognito)
            })
            .map(|running| running.stored.pane.pane_id.clone())
    }

    fn focus_open_web_pane(
        &mut self,
        connection: &str,
        params: &Value,
        scope: &str,
        workspace_id: &str,
        pane_id: &str,
    ) -> Result<Value> {
        let result = serde_json::to_value(WebOpenResult {
            pane_id: pane_id.into(),
        })
        .map_err(|_| errors::internal("Web open result serialization failed"))?;
        Ok(self
            .mutate(connection, "web.open", params, |change| {
                change.focus_pane(scope, workspace_id, pane_id)?;
                Ok((result, Vec::new()))
            })?
            .result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dispatch::Outcome, outbox};
    use serde_json::json;
    use soyeht_protocol::{ErrorKind, frame::Frame};
    use std::{fs, path::PathBuf};
    use uuid::Uuid;

    fn request(id: u64, method: &str, params: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
    }

    fn reply(core: &mut Core, connection: &str, request: Value) -> Value {
        let Outcome::Reply(value) = core.dispatch(connection, request).unwrap() else {
            panic!("expected reply")
        };
        value
    }

    async fn next_json(receiver: &mut outbox::Receiver) -> Value {
        let Frame::Json(bytes) = receiver.next().await.unwrap().unwrap().frame else {
            panic!("expected JSON frame")
        };
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn connect(
        core: &mut Core,
        client: &str,
        capabilities: Value,
    ) -> (String, outbox::Receiver) {
        let (sender, mut receiver) = outbox::channel();
        let connection = core.connect(true, false, sender).unwrap();
        let _ = next_json(&mut receiver).await;
        let welcome = reply(
            core,
            &connection,
            request(
                1,
                "hello",
                json!({"client":client,"clientVersion":"1",
                    "protocolVersion":1,"capabilities":capabilities}),
            ),
        );
        assert_eq!(welcome["role"], "human_local");
        (connection, receiver)
    }

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("soyeht-web-open-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[tokio::test]
    async fn web_open_normalizes_focuses_and_respects_explicit_new_panes() {
        let root = temp_root();
        let mut core = Core::open(&root).unwrap();
        let (connection, _receiver) = connect(&mut core, "web-navigation-test", json!([])).await;
        let workspace = reply(
            &mut core,
            &connection,
            request(
                2,
                "workspace.create",
                json!({"name":"Web","path":root,"idempotencyKey":"workspace"}),
            ),
        )["workspace"]["workspaceId"]
            .as_str()
            .unwrap()
            .to_owned();
        reply(
            &mut core,
            &connection,
            request(
                3,
                "workspace.focus",
                json!({"workspaceId":workspace,"idempotencyKey":"focus"}),
            ),
        );
        let first = reply(
            &mut core,
            &connection,
            request(
                4,
                "web.open",
                json!({"url":"localhost:5173","idempotencyKey":"open-one"}),
            ),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            core.panes[&first].stored.pane.url.as_deref(),
            Some("http://localhost:5173/")
        );
        let focused = reply(
            &mut core,
            &connection,
            request(
                5,
                "web.open",
                json!({"url":"http://localhost:5173","idempotencyKey":"focus-existing"}),
            ),
        );
        assert_eq!(focused["paneId"], first);
        assert_eq!(
            core.panes
                .values()
                .filter(|pane| pane.stored.pane.mode == PaneMode::Web)
                .count(),
            1
        );
        let second = reply(
            &mut core,
            &connection,
            request(
                6,
                "web.open",
                json!({"url":"example.com","newPane":true,"idempotencyKey":"open-two"}),
            ),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(second, first);
        assert_eq!(
            core.panes[&second].stored.pane.url.as_deref(),
            Some("https://example.com/")
        );
        let closed = reply(
            &mut core,
            &connection,
            request(
                7,
                "web.close",
                json!({"paneId":second,"idempotencyKey":"close"}),
            ),
        );
        assert_eq!(closed["paneId"], second);
        assert!(core.panes[&second].stored.closed);
        crate::test_paths::remove_after(core, root);
    }

    #[tokio::test]
    async fn web_open_refuses_unsupported_schemes_and_unavailable_human_mode() {
        let root = temp_root();
        let mut core = Core::open(&root).unwrap();
        let (connection, _receiver) = connect(&mut core, "web-refusal-test", json!([])).await;
        for (id, value, kind) in [
            (
                2,
                json!({"url":"mailto:a@example.com","idempotencyKey":"scheme"}),
                ErrorKind::NavigationRefused,
            ),
            (
                3,
                json!({"url":"example.com","mode":"human","idempotencyKey":"human"}),
                ErrorKind::WebHostUnavailable,
            ),
        ] {
            let Err(error) = core.dispatch(&connection, request(id, "web.open", value)) else {
                panic!("invalid web open should fail")
            };
            assert_eq!(errors::kind(&error), kind);
        }
        crate::test_paths::remove_after(core, root);
    }

    #[tokio::test]
    async fn web_navigate_routes_one_normalized_command_and_commits_its_receipt() {
        let root = temp_root();
        let mut core = Core::open(&root).unwrap();
        let (host, mut host_receiver) = connect(&mut core, "web-host", json!(["web_host"])).await;
        let (caller, mut caller_receiver) = connect(&mut core, "web-caller", json!([])).await;
        let workspace = reply(
            &mut core,
            &caller,
            request(
                2,
                "workspace.create",
                json!({"name":"Web","path":root,"idempotencyKey":"workspace"}),
            ),
        )["workspace"]["workspaceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let pane = reply(
            &mut core,
            &caller,
            request(
                3,
                "web.open",
                json!({"workspaceId":workspace,"url":"example.com",
                    "idempotencyKey":"open"}),
            ),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        let parameters = json!({"paneId":pane,"url":"localhost:5173",
            "idempotencyKey":"navigate"});
        let Outcome::Queued = core
            .dispatch(&caller, request(4, "web.navigate", parameters.clone()))
            .unwrap()
        else {
            panic!("web navigation should wait for its host")
        };
        let command = next_json(&mut host_receiver).await;
        assert_eq!(command["method"], "web.command");
        assert_eq!(command["params"]["command"], "navigate");
        assert_eq!(command["params"]["args"]["url"], "http://localhost:5173/");
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":command["id"],
                "result":{"paneId":pane}}),
        )
        .unwrap();
        let completed = next_json(&mut caller_receiver).await;
        assert_eq!(completed["id"], 4);
        assert_eq!(completed["result"], json!({"paneId":pane,"accepted":true}));
        let immediate = reply(&mut core, &caller, request(5, "web.navigate", parameters));
        assert_eq!(immediate, completed["result"]);
        crate::test_paths::remove_after(core, root);
    }
}
