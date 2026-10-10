use crate::{
    core::{Core, write_pane},
    errors::{self, Result},
};
use serde_json::Value;
use soyeht_protocol::{
    PaneMode, WebConsoleCountsEvent, WebHostConsoleCountsParams, WebHostConsoleCountsResult,
    WebHostNavigatedParams, WebHostNavigatedResult,
};
use soyeht_web::NormalizedUrl;

impl Core {
    pub(crate) fn web_host_update(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        self.require_web_host(connection)?;
        match method {
            "web.host.navigated" => self.web_host_navigated(connection, params),
            "web.host.console_counts" => self.web_host_console_counts(connection, params),
            _ => Err(errors::internal("Unknown web host update")),
        }
    }

    fn web_host_navigated(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: WebHostNavigatedParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid web navigation update"))?;
        let url = NormalizedUrl::parse(&request.url)?;
        let generation = request
            .navigation_generation
            .parse::<u32>()
            .ok()
            .filter(|generation| *generation > 0)
            .ok_or_else(|| errors::invalid("Invalid web navigation generation"))?;
        let mut saved = self.live_web_pane(&request.pane_id)?.clone();
        if generation < saved.pane.generation {
            return Err(errors::invalid("Web navigation generation moved backwards"));
        }
        saved.pane.generation = generation;
        saved.pane.url = Some(url.as_str().into());
        saved.pane.title = Some(request.title.clone());
        let result = serde_json::to_value(WebHostNavigatedResult { accepted: true })
            .map_err(|_| errors::internal("Web navigation update result serialization failed"))?;
        let event = serde_json::json!({
            "paneId": saved.pane.pane_id,
            "navigationGeneration": generation.to_string(),
            "url": url.as_str(),
            "title": request.title,
        });
        let mutation = self.mutate(connection, "web.host.navigated", params, |change| {
            write_pane(change, &saved)?;
            Ok((result, vec![("web.navigated".into(), event)]))
        })?;
        if !mutation.repeated {
            self.live_web_pane_mut(&request.pane_id)?.stored = saved;
        }
        Ok(mutation.result)
    }

    fn web_host_console_counts(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: WebHostConsoleCountsParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid web console count update"))?;
        self.live_web_pane(&request.pane_id)?;
        let result = serde_json::to_value(WebHostConsoleCountsResult { accepted: true })
            .map_err(|_| errors::internal("Web console count result serialization failed"))?;
        let event = serde_json::to_value(WebConsoleCountsEvent {
            pane_id: request.pane_id,
            warnings: request.warnings,
            errors: request.errors,
        })
        .map_err(|_| errors::internal("Web console count event serialization failed"))?;
        let mutation = self.mutate(connection, "web.host.console_counts", params, |_| {
            Ok((result, Vec::new()))
        })?;
        if !mutation.repeated {
            self.transient_event("web.console_counts", event)?;
        }
        Ok(mutation.result)
    }

    fn require_web_host(&self, connection: &str) -> Result<()> {
        if self.clients.get(connection).is_some_and(|client| {
            client.web_host && client.actor.is_some() && !client.outbox.is_closed()
        }) {
            Ok(())
        } else {
            Err(errors::unauthorized("Web host capability required"))
        }
    }

    fn live_web_pane(&self, pane_id: &str) -> Result<&crate::model::StoredPane> {
        self.panes
            .get(pane_id)
            .filter(|running| !running.stored.closed && running.stored.pane.mode == PaneMode::Web)
            .map(|running| &running.stored)
            .ok_or_else(|| errors::not_found("Web pane not found"))
    }

    fn live_web_pane_mut(&mut self, pane_id: &str) -> Result<&mut crate::model::RunningPane> {
        self.panes
            .get_mut(pane_id)
            .filter(|running| !running.stored.closed && running.stored.pane.mode == PaneMode::Web)
            .ok_or_else(|| errors::internal("Web pane disappeared after its update committed"))
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

    async fn connect(core: &mut Core, name: &str, host: bool) -> (String, outbox::Receiver) {
        let (sender, mut receiver) = outbox::channel();
        let connection = core.connect(true, false, sender).unwrap();
        let _ = next_json(&mut receiver).await;
        reply(
            core,
            &connection,
            request(
                1,
                "hello",
                json!({"client":name,"clientVersion":"1","protocolVersion":1,
                    "capabilities": if host { json!(["web_host"]) } else { json!([]) }}),
            ),
        );
        (connection, receiver)
    }

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("soyeht-web-state-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn workspace_and_pane(core: &mut Core, connection: &str, root: &PathBuf) -> String {
        let workspace = reply(
            core,
            connection,
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
            core,
            connection,
            request(
                3,
                "web.open",
                json!({"workspaceId":workspace,"url":"example.invalid",
                    "idempotencyKey":"open"}),
            ),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[tokio::test]
    async fn only_the_host_updates_durable_navigation_and_restart_state() {
        let root = temp_root();
        let mut core = Core::open(&root).unwrap();
        let (host, _host_receiver) = connect(&mut core, "host", true).await;
        let (viewer, _viewer_receiver) = connect(&mut core, "viewer", false).await;
        let pane = workspace_and_pane(&mut core, &viewer, &root);
        let params = json!({"paneId":pane,"navigationGeneration":"2",
            "url":"localhost:4173","title":"Local app","idempotencyKey":"navigated"});
        let Err(error) = core.dispatch(&viewer, request(4, "web.host.navigated", params.clone()))
        else {
            panic!("a non-host update should fail")
        };
        assert_eq!(errors::kind(&error), ErrorKind::Unauthorized);
        let result = reply(
            &mut core,
            &host,
            request(5, "web.host.navigated", params.clone()),
        );
        assert_eq!(result, json!({"accepted":true}));
        assert_eq!(
            reply(&mut core, &host, request(6, "web.host.navigated", params)),
            result
        );
        let Err(error) = core.dispatch(
            &host,
            request(
                7,
                "web.host.navigated",
                json!({"paneId":pane,"navigationGeneration":"1",
                    "url":"https://stale.invalid/","title":"Stale",
                    "idempotencyKey":"stale-navigation"}),
            ),
        ) else {
            panic!("an older navigation generation should fail")
        };
        assert_eq!(errors::kind(&error), ErrorKind::InvalidParams);
        assert_eq!(core.panes[&pane].stored.pane.generation, 2);
        assert_eq!(
            core.panes[&pane].stored.pane.url.as_deref(),
            Some("http://localhost:4173/")
        );
        assert_eq!(
            core.store
                .replay(0)
                .unwrap()
                .1
                .iter()
                .filter(|event| event.method == "web.navigated")
                .count(),
            1
        );
        drop(core);
        let restored = Core::open(&root).unwrap();
        assert_eq!(
            restored.panes[&pane].stored.pane.title.as_deref(),
            Some("Local app")
        );
        assert_eq!(restored.panes[&pane].stored.pane.generation, 2);
        crate::test_paths::remove_after(restored, root);
    }

    #[tokio::test]
    async fn console_counts_are_host_only_live_only_and_idempotent() {
        let root = temp_root();
        let mut core = Core::open(&root).unwrap();
        let (host, _host_receiver) = connect(&mut core, "host", true).await;
        let (viewer, mut viewer_receiver) = connect(&mut core, "viewer", false).await;
        let pane = workspace_and_pane(&mut core, &viewer, &root);
        core.clients.get_mut(&viewer).unwrap().events = true;
        let params = json!({"paneId":pane,"warnings":2,"errors":1,
            "idempotencyKey":"console"});
        assert_eq!(
            reply(
                &mut core,
                &host,
                request(4, "web.host.console_counts", params.clone()),
            ),
            json!({"accepted":true})
        );
        let event = next_json(&mut viewer_receiver).await;
        assert_eq!(event["method"], "web.console_counts");
        assert!(event["params"].get("eventId").is_none());
        assert_eq!(event["params"]["warnings"], 2);
        assert_eq!(
            reply(
                &mut core,
                &host,
                request(5, "web.host.console_counts", params),
            ),
            json!({"accepted":true})
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), viewer_receiver.next(),)
                .await
                .is_err()
        );
        assert!(
            core.store
                .replay(0)
                .unwrap()
                .1
                .iter()
                .all(|event| event.method != "web.console_counts")
        );
        crate::test_paths::remove_after(core, root);
    }
}
