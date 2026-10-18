use crate::{
    core::Core,
    errors::{self, Result},
    model,
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::{
    AttachmentRef, ErrorKind, RpcError, WebActionResult, WebAgentDrivingEvent, WebCloseResult,
    WebCommandKind, WebCommandParams, WebCommandResult, WebConsoleResult, WebEvaluateResult,
    WebNavigateResult, WebNetworkResult, WebResizeResult, WebScreenshotResult, WebSnapshotResult,
    WebWaitResult, attachment::attachment_frames, error_code, event, frame::Frame, method::*,
};
use soyeht_web::{command_response_budget, validate_command};
use uuid::Uuid;

const WEB_WAITER_LIMIT: usize = 128;

pub struct WebWaiter {
    pub(super) host: String,
    pub(super) driving: bool,
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
    pub(crate) fn attachment_chunk(
        &mut self,
        connection: &str,
        header: [u8; 16],
        bytes: &[u8],
    ) -> Result<()> {
        let client = self
            .clients
            .get_mut(connection)
            .filter(|client| client.actor.is_some())
            .ok_or_else(|| errors::unauthorized("Attachment sender is not authenticated"))?;
        client.attachments.push(header, bytes)?;
        Ok(())
    }

    pub(crate) fn web_attachment_pending(&self, connection: &str) -> Result<bool> {
        let client = self
            .clients
            .get(connection)
            .ok_or_else(|| errors::unauthorized("Attachment connection is unknown"))?;
        Ok(client.attachments.is_pending())
    }

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
                driving: false,
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
        let mut attachment = None;
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
            let reference = web_attachment_reference(waiter, &result)?;
            let public = public_result(waiter, &result)?;
            attachment = self.commit_web_attachment(host, reference)?;
            Ok(public)
        } else {
            if self.web_attachment_pending(host)? {
                return Err(errors::invalid(
                    "Web host error followed an uncommitted attachment",
                ));
            }
            let error: RpcError = serde_json::from_value(response["error"].clone())
                .map_err(|_| errors::invalid("Invalid web command error"))?;
            validate_host_error(&error)?;
            Err(error)
        };
        let waiter = errors::invariant(
            self.web_waiters.remove(id),
            "Web command waiter disappeared",
        )?;
        self.finish_web_driving(&waiter)?;
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
                    Ok(value) => model::response(&waiter.method, request, value.clone())?,
                    Err(error) => model::failure(request, error.clone())?,
                };
                send_web_response(&client.outbox, attachment.as_ref(), frame)?;
            }
        }
        Ok(())
    }

    fn commit_web_attachment(
        &mut self,
        host: &str,
        reference: Option<AttachmentRef>,
    ) -> Result<Option<(AttachmentRef, Vec<u8>)>> {
        let client = self
            .clients
            .get_mut(host)
            .ok_or_else(|| errors::unauthorized("Web host disconnected"))?;
        match reference {
            Some(reference) => {
                validate_web_attachment(&reference)?;
                let bytes = client.attachments.commit(&reference)?;
                Ok(Some((reference, bytes)))
            }
            None if client.attachments.is_pending() => Err(errors::invalid(
                "Web host result did not commit its pending attachment",
            )),
            None => Ok(None),
        }
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
        self.finish_web_driving(&waiter)?;
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
        let frame = model::request(json!(id), WEB_COMMAND, payload)?;
        let sent = self
            .clients
            .get(&host)
            .is_some_and(|client| client.outbox.send(frame));
        if !sent {
            return Err(errors::web_host_unavailable("Web host disconnected"));
        }
        self.begin_web_driving(id)?;
        Ok(())
    }

    fn begin_web_driving(&mut self, id: &str) -> Result<()> {
        let Some((pane_id, domain)) = self.web_waiters.get(id).and_then(|waiter| {
            waiter
                .permission
                .driving_domain()
                .map(|domain| (waiter.pane_id.clone(), domain.to_owned()))
        }) else {
            return Ok(());
        };
        let already_on = self
            .web_waiters
            .values()
            .any(|waiter| waiter.pane_id == pane_id && waiter.driving);
        self.web_waiters
            .get_mut(id)
            .ok_or_else(|| errors::internal("Web command waiter disappeared"))?
            .driving = true;
        if !already_on {
            self.publish_web_driving(&pane_id, true, Some(domain))?;
        }
        Ok(())
    }

    pub(super) fn finish_web_driving(&self, waiter: &WebWaiter) -> Result<()> {
        if waiter.driving
            && !self
                .web_waiters
                .values()
                .any(|other| other.pane_id == waiter.pane_id && other.driving)
        {
            self.publish_web_driving(&waiter.pane_id, false, None)?;
        }
        Ok(())
    }

    fn publish_web_driving(&self, pane_id: &str, on: bool, domain: Option<String>) -> Result<()> {
        let event = WebAgentDrivingEvent {
            pane_id: pane_id.into(),
            on,
            domain,
        };
        let params = serde_json::to_value(event)
            .map_err(|_| errors::internal("Web driving event serialization failed"))?;
        self.transient_event(event::WEB_AGENT_DRIVING, params)
    }
}

fn public_result(waiter: &WebWaiter, result: &WebCommandResult) -> Result<Value> {
    match waiter.method.as_str() {
        WEB_NAVIGATE => serde_json::to_value(WebNavigateResult {
            pane_id: waiter.pane_id.clone(),
            accepted: true,
        })
        .map_err(|_| errors::internal("Web navigation result serialization failed")),
        WEB_SNAPSHOT => typed_public_result::<WebSnapshotResult>(waiter, result),
        WEB_CLICK | WEB_TYPE | WEB_PRESS | WEB_HOVER | WEB_SELECT | WEB_SCROLL => {
            typed_public_result::<WebActionResult>(waiter, result)
        }
        WEB_WAIT => typed_public_result::<WebWaitResult>(waiter, result),
        WEB_EVALUATE => typed_public_result::<WebEvaluateResult>(waiter, result),
        WEB_SCREENSHOT => typed_public_result::<WebScreenshotResult>(waiter, result),
        WEB_CONSOLE => typed_public_result::<WebConsoleResult>(waiter, result),
        WEB_NETWORK => typed_public_result::<WebNetworkResult>(waiter, result),
        WEB_RESIZE => typed_public_result::<WebResizeResult>(waiter, result),
        WEB_CLOSE => typed_public_result::<WebCloseResult>(waiter, result),
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

fn web_attachment_reference(
    waiter: &WebWaiter,
    result: &WebCommandResult,
) -> Result<Option<AttachmentRef>> {
    let field = match waiter.method.as_str() {
        WEB_SCREENSHOT => Some("image"),
        WEB_SNAPSHOT => Some("screenshot"),
        _ => None,
    };
    let Some(field) = field else {
        return Ok(None);
    };
    let Some(value) = result.value.as_ref().and_then(|value| value.get(field)) else {
        return if waiter.method == WEB_SCREENSHOT {
            Err(errors::invalid("Web screenshot result omitted its image"))
        } else {
            Ok(None)
        };
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|_| errors::invalid("Invalid web screenshot attachment reference"))
}

fn validate_web_attachment(reference: &AttachmentRef) -> Result<()> {
    if reference.mime_type != "image/png" || reference.name != "screenshot.png" {
        return Err(errors::invalid(
            "Web screenshot attachment must be a named PNG image",
        ));
    }
    Ok(())
}

fn send_web_response(
    outbox: &crate::outbox::Outbox,
    attachment: Option<&(AttachmentRef, Vec<u8>)>,
    response: Frame,
) -> Result<()> {
    let Some((reference, bytes)) = attachment else {
        outbox.send(response);
        return Ok(());
    };
    for frame in attachment_frames(reference, bytes)? {
        if !outbox.send_ordered(frame) {
            return Ok(());
        }
    }
    outbox.send_ordered(response);
    Ok(())
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
mod tests;
