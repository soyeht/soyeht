use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    ErrorKind, RpcError, WebCommandKind, WebCommandParams, WebCommandResult, error_code,
};
use soyeht_web::{command_response_budget, validate_command};
use uuid::Uuid;

const WEB_WAITER_LIMIT: usize = 128;

pub struct WebWaiter {
    host: String,
    caller: String,
    caller_request: Value,
    pane_id: String,
    command: WebCommandKind,
    deadline_ms: i64,
}

impl Core {
    pub fn queue_web_command(
        &mut self,
        caller: &str,
        caller_request: Value,
        params: WebCommandParams,
    ) -> Result<String> {
        validate_command(&params, soyeht_protocol::WebInteractionMode::Structured)?;
        let caller_live = self
            .clients
            .get(caller)
            .is_some_and(|client| client.actor.is_some() && !client.outbox.is_closed());
        if !caller_live {
            return Err(errors::unauthorized("Web command caller is disconnected"));
        }
        let pane_live = self.panes.get(&params.pane_id).is_some_and(|running| {
            !running.stored.closed && running.stored.pane.mode == soyeht_protocol::PaneMode::Web
        });
        if !pane_live {
            return Err(errors::not_found("Web pane not found"));
        }
        if self.web_waiters.len() >= WEB_WAITER_LIMIT {
            return Err(errors::pane_busy("Web command waiter limit reached"));
        }
        let host = self
            .clients
            .iter()
            .find(|(_, client)| {
                client.web_host && client.actor.is_some() && !client.outbox.is_closed()
            })
            .map(|(id, _)| id.clone())
            .ok_or_else(|| errors::web_host_unavailable("No web host is connected"))?;
        let reverse_id = Uuid::new_v4().to_string();
        let deadline_ms = self.now_ms().saturating_add(i64::from(params.timeout_ms));
        let pane_id = params.pane_id.clone();
        let command = params.command.clone();
        let payload = serde_json::to_value(params)
            .map_err(|_| errors::internal("Web command serialization failed"))?;
        let frame = model::request(json!(reverse_id), "web.command", payload)?;
        self.web_waiters.insert(
            reverse_id.clone(),
            WebWaiter {
                host: host.clone(),
                caller: caller.into(),
                caller_request,
                pane_id,
                command,
                deadline_ms,
            },
        );
        let sent = self
            .clients
            .get(&host)
            .is_some_and(|client| client.outbox.send(frame));
        if !sent {
            self.web_waiters.remove(&reverse_id);
            return Err(errors::web_host_unavailable("Web host disconnected"));
        }
        Ok(reverse_id)
    }

    pub(crate) fn web_host_response(&mut self, host: &str, response: Value) -> Result<()> {
        let id = valid_response_id(&response)?;
        let waiter = self
            .web_waiters
            .get(id)
            .ok_or_else(|| errors::invalid("Unknown web command response"))?;
        if waiter.host != host {
            return Err(errors::unauthorized(
                "Web command response belongs to another host",
            ));
        }
        let frame = if response.get("result").is_some() {
            let result: WebCommandResult = serde_json::from_value(response["result"].clone())
                .map_err(|_| errors::invalid("Invalid web command result"))?;
            if result.pane_id != waiter.pane_id {
                return Err(errors::invalid("Web command result changed pane identity"));
            }
            let bytes = serde_json::to_vec(&result)
                .map_err(|_| errors::invalid("Invalid web command result"))?;
            if bytes.len() > command_response_budget(&waiter.command) {
                return Err(errors::invalid(
                    "Web command result exceeds its byte budget",
                ));
            }
            model::response(waiter.caller_request.clone(), response["result"].clone())?
        } else {
            let error: RpcError = serde_json::from_value(response["error"].clone())
                .map_err(|_| errors::invalid("Invalid web command error"))?;
            validate_host_error(&error)?;
            model::failure(waiter.caller_request.clone(), error)?
        };
        let waiter = errors::invariant(
            self.web_waiters.remove(id),
            "Web command waiter disappeared",
        )?;
        if let Some(client) = self.clients.get(&waiter.caller) {
            client.outbox.send(frame);
        }
        Ok(())
    }

    pub(crate) fn disconnect_web_host(&mut self, host: &str) -> Result<()> {
        let ids: Vec<_> = self
            .web_waiters
            .iter()
            .filter(|(_, waiter)| waiter.host == host)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.fail_web_waiter(&id, "Web host disconnected")?;
        }
        Ok(())
    }

    pub(crate) fn advance_web_commands(&mut self) -> Result<()> {
        let now = self.now_ms();
        let expired: Vec<_> = self
            .web_waiters
            .iter()
            .filter(|(_, waiter)| waiter.deadline_ms <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.fail_web_waiter(&id, "Web host command timed out")?;
        }
        Ok(())
    }

    fn fail_web_waiter(&mut self, id: &str, message: &str) -> Result<()> {
        let Some(waiter) = self.web_waiters.remove(id) else {
            return Ok(());
        };
        let error = errors::wire(errors::web_host_unavailable(message));
        if let Some(client) = self.clients.get(&waiter.caller) {
            client
                .outbox
                .send(model::failure(waiter.caller_request, error)?);
        }
        Ok(())
    }
}

fn valid_response_id(response: &Value) -> Result<&str> {
    let valid_envelope = response["jsonrpc"] == "2.0"
        && response.get("method").is_none()
        && response["id"].is_string()
        && (response.get("result").is_some() ^ response.get("error").is_some());
    if !valid_envelope {
        return Err(errors::invalid("Invalid web command response envelope"));
    }
    response["id"]
        .as_str()
        .ok_or_else(|| errors::invalid("Invalid web command response id"))
}

fn validate_host_error(error: &RpcError) -> Result<()> {
    let allowed = matches!(
        error.data.kind,
        ErrorKind::StaleReference
            | ErrorKind::HumanTookOver
            | ErrorKind::NavigationRefused
            | ErrorKind::InternalError
    );
    if !allowed || error.code != error_code(&error.data.kind) {
        return Err(errors::invalid("Invalid web command error"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dispatch::Outcome, outbox, ports::testing::FixedClock};
    use serde_json::json;
    use soyeht_protocol::frame::Frame;
    use std::{fs, path::PathBuf, sync::Arc};

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

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("soyeht-web-host-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    async fn connect(core: &mut Core, capabilities: Value) -> (String, outbox::Receiver) {
        let (sender, mut receiver) = outbox::channel();
        let connection = core.connect(true, sender).unwrap();
        let challenge = next_json(&mut receiver).await;
        assert_eq!(challenge["method"], "auth.challenge");
        let welcome = reply(
            core,
            &connection,
            request(
                1,
                "hello",
                json!({
                    "client":"web-host-test","clientVersion":"1","protocolVersion":1,
                    "capabilities":capabilities
                }),
            ),
        );
        assert_eq!(welcome["role"], "human_local");
        (connection, receiver)
    }

    #[tokio::test]
    async fn one_local_host_routes_typed_bounded_results_and_disconnects_fail_waiters() {
        let root = temp_root();
        let mut core = Core::open(&root).unwrap();
        core.clock = Arc::new(FixedClock(10));
        let (host, mut host_receiver) = connect(&mut core, json!(["web_host"])).await;
        let (caller, mut caller_receiver) = connect(&mut core, json!([])).await;
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
                "pane.create",
                json!({"workspaceId":workspace,"mode":"web","idempotencyKey":"pane"}),
            ),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        let params: WebCommandParams = serde_json::from_value(json!({
            "paneId":pane,"command":"snapshot","args":{},"timeoutMs":100,
            "operationKey":"operation"
        }))
        .unwrap();
        let command_id = core
            .queue_web_command(&caller, json!(44), params.clone())
            .unwrap();
        let command = next_json(&mut host_receiver).await;
        assert_eq!(command["id"], command_id);
        assert_eq!(command["method"], "web.command");
        assert_eq!(command["params"]["paneId"], pane);
        assert!(
            core.web_host_response(
                &host,
                json!({"jsonrpc":"2.0","id":command_id,"result":{"paneId":"wrong"}}),
            )
            .is_err()
        );
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":command_id,"result":{"paneId":pane,"value":[]}}),
        )
        .unwrap();
        let response = next_json(&mut caller_receiver).await;
        assert_eq!(response["id"], 44);
        assert_eq!(response["result"]["paneId"], pane);

        core.queue_web_command(&caller, json!(45), params.clone())
            .unwrap();
        let _ = next_json(&mut host_receiver).await;
        core.clock = Arc::new(FixedClock(111));
        core.advance_web_commands().unwrap();
        let timeout = next_json(&mut caller_receiver).await;
        assert_eq!(timeout["id"], 45);
        assert_eq!(timeout["error"]["data"]["kind"], "web_host_unavailable");

        core.clock = Arc::new(FixedClock(200));
        core.queue_web_command(&caller, json!(46), params).unwrap();
        let _ = next_json(&mut host_receiver).await;
        core.disconnect_web_host(&host).unwrap();
        let disconnected = next_json(&mut caller_receiver).await;
        assert_eq!(disconnected["id"], 46);
        assert_eq!(
            disconnected["error"]["data"]["kind"],
            "web_host_unavailable"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_second_live_host_and_nonlocal_host_claims_are_rejected() {
        let root = temp_root();
        let mut core = Core::open(&root).unwrap();
        let (_host, _host_receiver) = connect(&mut core, json!(["web_host"])).await;
        let (sender, mut receiver) = outbox::channel();
        let second = core.connect(true, sender).unwrap();
        let _ = next_json(&mut receiver).await;
        let duplicate = core.dispatch(
            &second,
            request(
                1,
                "hello",
                json!({"client":"second","clientVersion":"1","protocolVersion":1,
                    "capabilities":["web_host"]}),
            ),
        );
        let Err(duplicate) = duplicate else {
            panic!("second web host should be rejected")
        };
        assert_eq!(errors::kind(&duplicate), ErrorKind::Unauthorized);
        let (sender, mut receiver) = outbox::channel();
        let remote = core.connect(false, sender).unwrap();
        let _ = next_json(&mut receiver).await;
        let rejected = core.dispatch(
            &remote,
            request(
                1,
                "hello",
                json!({"client":"remote","clientVersion":"1","protocolVersion":1,
                    "deviceToken":"unused","capabilities":["web_host"]}),
            ),
        );
        let Err(rejected) = rejected else {
            panic!("remote web host should be rejected")
        };
        assert_eq!(errors::kind(&rejected), ErrorKind::Unauthorized);
        fs::remove_dir_all(root).unwrap();
    }
}
