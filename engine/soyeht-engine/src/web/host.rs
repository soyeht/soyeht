use crate::{
    core::Core,
    errors::{self, Result},
    model,
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::{
    ErrorKind, RpcError, WebActionResult, WebCloseResult, WebCommandKind, WebCommandParams,
    WebCommandResult, WebConsoleResult, WebEvaluateResult, WebNavigateResult, WebNetworkResult,
    WebResizeResult, WebScreenshotResult, WebSnapshotResult, WebWaitResult, error_code,
};
use soyeht_web::{command_response_budget, validate_command};
use uuid::Uuid;

const WEB_WAITER_LIMIT: usize = 128;

pub struct WebWaiter {
    pub(super) host: String,
    pub(super) callers: Vec<(String, Value)>,
    pub(super) pane_id: String,
    pub(super) command: WebCommandKind,
    pub(super) deadline_ms: i64,
    pub(super) method: String,
    pub(super) mutation: Option<WebMutation>,
    pub(super) params: WebCommandParams,
    pub(super) permission: super::permissions::WebPermission,
}

pub(super) struct WebMutation {
    scope: String,
    key: String,
    fingerprint: String,
    actor: Value,
}

impl Core {
    pub fn queue_web_command(
        &mut self,
        caller: &str,
        caller_request: Value,
        method: &str,
        raw_params: &Value,
        params: WebCommandParams,
    ) -> Result<String> {
        validate_command(&params, soyeht_protocol::WebInteractionMode::Structured)?;
        let client = self
            .clients
            .get(caller)
            .filter(|client| client.actor.is_some() && !client.outbox.is_closed())
            .ok_or_else(|| errors::unauthorized("Web command caller is disconnected"))?;
        let mutation = command_mutation(client, method, raw_params, &params)?;
        if let Some((id, waiter)) = mutation.as_ref().and_then(|candidate| {
            self.web_waiters.iter_mut().find(|(_, waiter)| {
                waiter.mutation.as_ref().is_some_and(|existing| {
                    existing.scope == candidate.scope && existing.key == candidate.key
                })
            })
        }) {
            let candidate = mutation
                .as_ref()
                .ok_or_else(|| errors::internal("Web mutation disappeared"))?;
            if waiter
                .mutation
                .as_ref()
                .is_none_or(|existing| existing.fingerprint != candidate.fingerprint)
            {
                return Err(errors::idempotency_conflict(
                    "Web operation key belongs to another mutation",
                ));
            }
            if !waiter
                .callers
                .iter()
                .any(|(owner, request)| owner == caller && request == &caller_request)
            {
                if waiter.callers.len() >= WEB_WAITER_LIMIT {
                    return Err(errors::pane_busy("Web operation waiter limit reached"));
                }
                waiter.callers.push((caller.into(), caller_request));
            }
            return Ok(id.clone());
        }
        let permission = self.web_permission_plan(caller, method, &params)?;
        let pane_live = self.panes.get(&params.pane_id).is_some_and(|running| {
            !running.stored.closed && running.stored.pane.mode == soyeht_protocol::PaneMode::Web
        });
        if !pane_live {
            return Err(errors::not_found("Web pane not found"));
        }
        if self.web_waiters.len() >= WEB_WAITER_LIMIT {
            return Err(errors::pane_busy("Web command waiter limit reached"));
        }
        let reverse_id = Uuid::new_v4().to_string();
        let deadline_ms = permission
            .expires_ms
            .unwrap_or_else(|| self.now_ms().saturating_add(i64::from(params.timeout_ms)));
        let pane_id = params.pane_id.clone();
        let command = params.command.clone();
        self.web_waiters.insert(
            reverse_id.clone(),
            WebWaiter {
                host: String::new(),
                callers: vec![(caller.into(), caller_request)],
                pane_id,
                command,
                deadline_ms,
                method: method.into(),
                mutation,
                params,
                permission,
            },
        );
        if self.web_waiters[&reverse_id]
            .permission
            .request_id
            .is_none()
            && let Err(error) = self.send_web_waiter(&reverse_id)
        {
            self.web_waiters.remove(&reverse_id);
            return Err(error);
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
        let result = if response.get("result").is_some() {
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
            if result.cookies_present == Some(true) {
                return self.web_session_required(id);
            }
            Ok(public_result(waiter, &result)?)
        } else {
            let error: RpcError = serde_json::from_value(response["error"].clone())
                .map_err(|_| errors::invalid("Invalid web command error"))?;
            validate_host_error(&error)?;
            Err(error)
        };
        let waiter = errors::invariant(
            self.web_waiters.remove(id),
            "Web command waiter disappeared",
        )?;
        let result = match result {
            Ok(result) if waiter.mutation.is_some() => {
                let mutation = waiter
                    .mutation
                    .as_ref()
                    .ok_or_else(|| errors::internal("Web mutation disappeared"))?;
                let mutation = StorePort::mutate_as(
                    &mut self.store,
                    &mutation.scope,
                    &mutation.key,
                    &mutation.fingerprint,
                    Some(&mutation.actor),
                    |_| Ok((result, Vec::new())),
                );
                match mutation {
                    Ok(mutation) => Ok(mutation.result),
                    Err(error) => Err(errors::wire(errors::storage(error))),
                }
            }
            Ok(result) => Ok(result),
            Err(error) => Err(error),
        };
        for (caller, request) in waiter.callers {
            if let Some(client) = self.clients.get(&caller) {
                let frame = match &result {
                    Ok(value) => model::response(request, value.clone())?,
                    Err(error) => model::failure(request, error.clone())?,
                };
                client.outbox.send(frame);
            }
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
        self.advance_web_permissions()?;
        let now = self.now_ms();
        let expired: Vec<_> = self
            .web_waiters
            .iter()
            .filter(|(_, waiter)| {
                waiter.permission.request_id.is_none() && waiter.deadline_ms <= now
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.fail_web_waiter(&id, "Web host command timed out")?;
        }
        Ok(())
    }

    pub(super) fn fail_web_waiter(&mut self, id: &str, message: &str) -> Result<()> {
        let Some(waiter) = self.web_waiters.remove(id) else {
            return Ok(());
        };
        let error = errors::wire(errors::web_host_unavailable(message));
        for (caller, request) in waiter.callers {
            if let Some(client) = self.clients.get(&caller) {
                client.outbox.send(model::failure(request, error.clone())?);
            }
        }
        Ok(())
    }

    pub(super) fn send_web_waiter(&mut self, id: &str) -> Result<()> {
        let host = self
            .clients
            .iter()
            .find(|(_, client)| {
                client.web_host && client.actor.is_some() && !client.outbox.is_closed()
            })
            .map(|(id, _)| id.clone())
            .ok_or_else(|| errors::web_host_unavailable("No web host is connected"))?;
        let waiter = self
            .web_waiters
            .get_mut(id)
            .ok_or_else(|| errors::internal("Web command waiter disappeared"))?;
        waiter.host.clone_from(&host);
        waiter.permission.apply_to(&mut waiter.params)?;
        let payload = serde_json::to_value(&waiter.params)
            .map_err(|_| errors::internal("Web command serialization failed"))?;
        let frame = model::request(json!(id), "web.command", payload)?;
        let sent = self
            .clients
            .get(&host)
            .is_some_and(|client| client.outbox.send(frame));
        if !sent {
            return Err(errors::web_host_unavailable("Web host disconnected"));
        }
        Ok(())
    }
}

fn public_result(waiter: &WebWaiter, result: &WebCommandResult) -> Result<Value> {
    match waiter.method.as_str() {
        "web.navigate" => serde_json::to_value(WebNavigateResult {
            pane_id: waiter.pane_id.clone(),
            accepted: true,
        })
        .map_err(|_| errors::internal("Web navigation result serialization failed")),
        "web.snapshot" => typed_public_result::<WebSnapshotResult>(waiter, result),
        "web.click" | "web.type" | "web.press" | "web.hover" | "web.select" | "web.scroll" => {
            typed_public_result::<WebActionResult>(waiter, result)
        }
        "web.wait" => typed_public_result::<WebWaitResult>(waiter, result),
        "web.evaluate" => typed_public_result::<WebEvaluateResult>(waiter, result),
        "web.screenshot" => typed_public_result::<WebScreenshotResult>(waiter, result),
        "web.console" => typed_public_result::<WebConsoleResult>(waiter, result),
        "web.network" => typed_public_result::<WebNetworkResult>(waiter, result),
        "web.resize" => typed_public_result::<WebResizeResult>(waiter, result),
        "web.close" => typed_public_result::<WebCloseResult>(waiter, result),
        _ => Err(errors::internal("Unsupported routed web result")),
    }
}

fn typed_public_result<T: serde::de::DeserializeOwned + serde::Serialize>(
    waiter: &WebWaiter,
    result: &WebCommandResult,
) -> Result<Value> {
    let mut object = result
        .value
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| errors::invalid("Web command result value must be an object"))?;
    if object
        .insert("paneId".into(), json!(waiter.pane_id))
        .is_some()
    {
        return Err(errors::invalid(
            "Web command result value contains pane identity",
        ));
    }
    let typed: T = serde_json::from_value(Value::Object(object))
        .map_err(|_| errors::invalid("Invalid typed web command result"))?;
    let canonical = serde_json::to_value(typed)
        .map_err(|_| errors::internal("Web command result serialization failed"))?;
    soyeht_protocol::validate::result(&waiter.method, &canonical)
        .map_err(|_| errors::invalid("Invalid typed web command result"))?;
    Ok(canonical)
}

fn command_mutation(
    client: &model::Connected,
    method: &str,
    raw_params: &Value,
    params: &WebCommandParams,
) -> Result<Option<WebMutation>> {
    let Some(key) = raw_params.get("idempotencyKey").and_then(Value::as_str) else {
        return Ok(None);
    };
    if params.operation_key != key {
        return Err(errors::invalid("Web operation identity changed"));
    }
    let actor = serde_json::to_value(
        client
            .actor
            .as_ref()
            .ok_or_else(|| errors::unauthorized("Web command caller is disconnected"))?,
    )
    .map_err(|_| errors::internal("Web command actor serialization failed"))?;
    Ok(Some(WebMutation {
        scope: client.scope.clone(),
        key: key.into(),
        fingerprint: STANDARD.encode(crate::auth::token_hash(
            &json!([method, raw_params]).to_string(),
        )),
        actor,
    }))
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

    fn command_params(pane_id: &str, key: &str) -> WebCommandParams {
        serde_json::from_value(json!({
            "paneId":pane_id,"command":"navigate","args":{"action":"reload"},
            "timeoutMs":100,"operationKey":key
        }))
        .unwrap()
    }

    fn navigate_params(pane_id: &str, key: &str) -> Value {
        json!({"paneId":pane_id,"action":"reload","idempotencyKey":key})
    }

    fn human_args(args: Value) -> Value {
        let mut args = args.as_object().unwrap().clone();
        args.insert(
            "_soyehtAccess".into(),
            json!({"session":"human","evaluation":false}),
        );
        Value::Object(args)
    }

    fn assert_human_args(command: &Value, args: Value) {
        assert_eq!(command["params"]["args"], human_args(args));
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
        let raw = navigate_params(&pane, "operation");
        let params = command_params(&pane, "operation");
        let command_id = core
            .queue_web_command(&caller, json!(44), "web.navigate", &raw, params.clone())
            .unwrap();
        let repeated_id = core
            .queue_web_command(&caller, json!(440), "web.navigate", &raw, params)
            .unwrap();
        assert_eq!(repeated_id, command_id);
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
        assert_eq!(response["result"], json!({"paneId":pane,"accepted":true}));
        let repeated = next_json(&mut caller_receiver).await;
        assert_eq!(repeated["id"], 440);
        assert_eq!(repeated["result"], response["result"]);
        let Outcome::Reply(stored) = core
            .dispatch(
                &caller,
                request(441, "web.navigate", navigate_params(&pane, "operation")),
            )
            .unwrap()
        else {
            panic!("stored web navigation should reply immediately")
        };
        assert_eq!(stored, response["result"]);

        let raw = navigate_params(&pane, "timeout");
        core.queue_web_command(
            &caller,
            json!(45),
            "web.navigate",
            &raw,
            command_params(&pane, "timeout"),
        )
        .unwrap();
        let _ = next_json(&mut host_receiver).await;
        core.clock = Arc::new(FixedClock(111));
        core.advance_web_commands().unwrap();
        let timeout = next_json(&mut caller_receiver).await;
        assert_eq!(timeout["id"], 45);
        assert_eq!(timeout["error"]["data"]["kind"], "web_host_unavailable");

        core.clock = Arc::new(FixedClock(200));
        let raw = navigate_params(&pane, "disconnect");
        core.queue_web_command(
            &caller,
            json!(46),
            "web.navigate",
            &raw,
            command_params(&pane, "disconnect"),
        )
        .unwrap();
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

    #[tokio::test]
    async fn reads_are_fresh_and_mutations_are_typed_bounded_and_idempotent() {
        let root = temp_root();
        let mut core = Core::open(&root).unwrap();
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
        let Outcome::Queued = core
            .dispatch(
                &caller,
                request(4, "web.snapshot", json!({"paneId":pane,"screenshot":true})),
            )
            .unwrap()
        else {
            panic!("snapshot should wait for the host")
        };
        let snapshot_command = next_json(&mut host_receiver).await;
        assert_eq!(snapshot_command["params"]["command"], "snapshot");
        assert_human_args(&snapshot_command, json!({"screenshot":true}));
        assert!(snapshot_command["params"]["operationKey"].is_string());
        let oversized = "x".repeat(soyeht_protocol::limits::WEB_SNAPSHOT_BYTES);
        assert!(
            core.web_host_response(
                &host,
                json!({"jsonrpc":"2.0","id":snapshot_command["id"],"result":{
                    "paneId":pane,"value":{"navigationGeneration":"1","title":"Page",
                        "url":"https://example.test/","nodes":[],"truncated":false,
                        "unknown":oversized}}}),
            )
            .is_err()
        );
        assert!(
            core.web_host_response(
                &host,
                json!({"jsonrpc":"2.0","id":snapshot_command["id"],"result":{
                    "paneId":pane,"value":{"navigationGeneration":"1","title":"Page",
                        "url":"https://example.test/","truncated":false}}}),
            )
            .is_err()
        );
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":snapshot_command["id"],"result":{
                "paneId":pane,"value":{"navigationGeneration":"1","title":"Page",
                    "url":"https://example.test/","nodes":[],"truncated":false}}}),
        )
        .unwrap();
        let snapshot = next_json(&mut caller_receiver).await;
        assert_eq!(snapshot["result"]["title"], "Page");
        assert_eq!(snapshot["result"]["paneId"], pane);
        assert!(matches!(
            core.dispatch(
                &caller,
                request(5, "web.snapshot", json!({"paneId":pane,"screenshot":true})),
            )
            .unwrap(),
            Outcome::Queued
        ));
        let fresh_snapshot = next_json(&mut host_receiver).await;
        assert_ne!(fresh_snapshot["id"], snapshot_command["id"]);
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":fresh_snapshot["id"],"result":{
                "paneId":pane,"value":{"navigationGeneration":"1","title":"Page 2",
                    "url":"https://example.test/","nodes":[],"truncated":false}}}),
        )
        .unwrap();
        let _ = next_json(&mut caller_receiver).await;
        let click = json!({"paneId":pane,"ref":"e1","idempotencyKey":"click-1"});
        assert!(matches!(
            core.dispatch(&caller, request(6, "web.click", click.clone()))
                .unwrap(),
            Outcome::Queued
        ));
        let click_command = next_json(&mut host_receiver).await;
        assert_human_args(&click_command, json!({"ref":"e1"}));
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":click_command["id"],"result":{
                "paneId":pane,"value":{"completed":true}}}),
        )
        .unwrap();
        let clicked = next_json(&mut caller_receiver).await;
        assert_eq!(clicked["result"], json!({"paneId":pane,"completed":true}));
        let Outcome::Reply(repeated) = core
            .dispatch(&caller, request(7, "web.click", click))
            .unwrap()
        else {
            panic!("completed mutation should use its durable receipt")
        };
        assert_eq!(repeated, clicked["result"]);
        fs::remove_dir_all(root).unwrap();
    }
}
