use crate::{
    auth,
    core::Core,
    errors::{self, Result},
    model::{self, Connected},
    outbox::Outbox,
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_platform::log::{Failure as _, Line};
use soyeht_protocol::{
    Actor, AttachmentRef, ClientCapability, HelloParams, Role, Welcome,
    attachment::AttachmentAssembler, event, frame::Frame, limits, method,
};
use std::collections::BTreeMap;
use uuid::Uuid;

pub enum Outcome {
    Reply(Value),
    Catalog {
        method: String,
        params: Value,
    },
    /// Blocking work to run off the Core lock before the reply (ADR 0079).
    Offload(crate::offload::Offload),
    Attachment {
        result: Value,
        reference: AttachmentRef,
        bytes: Vec<u8>,
    },
    Queued,
    Snapshot {
        parts: Vec<Frame>,
        result: Value,
    },
}

fn requires_signed_ui(method: &str, signed_ui: bool) -> Result<()> {
    if matches!(
        method,
        method::PANE_EXPERIMENT_SET
            | method::PANE_EXPERIMENT_DELETE
            | method::PANE_EXPERIMENT_EXPORT
    ) && !signed_ui
    {
        return Err(errors::forbidden_role(
            "Pane-state experiment data can be recorded, deleted, or exported only in the signed Soyeht app on this Mac",
        ));
    }
    Ok(())
}

/// A client that reconnects with stale credentials leaves no other trace than
/// its refused handshakes. The record names the client it declared (only when
/// that is a plain label) and the engine's own fixed reason, never a credential.
fn refused_handshake_record(connection: &str, params: &Value, error: &errors::Error) -> Line {
    let client = params["client"].as_str().filter(|name| {
        (1..=32).contains(&name.len())
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    });
    Line::new("engine.handshake_refused")
        .field("connectionId", connection)
        .field_opt("client", client)
        .field("kind", error.failure_kind())
        .field("reason", error)
}

fn log_refused_handshake(connection: &str, params: &Value, error: &errors::Error) {
    refused_handshake_record(connection, params, error).emit();
}

fn uses_native_rpc_result(method: &str) -> bool {
    matches!(
        method,
        method::PANE_CONFIG_SET | method::PANE_HARNESS_SWITCH
    )
}

impl Core {
    pub fn connect(&mut self, local: bool, signed_ui: bool, outbox: Outbox) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let nonce = auth::random_bytes().map_err(|error| errors::internal(error.to_string()))?;
        outbox.send(model::notification(
            event::AUTH_CHALLENGE,
            json!({"nonce":STANDARD.encode(nonce),"engineId":self.store.db_id}),
        )?);
        self.clients.insert(
            id.clone(),
            Connected {
                outbox,
                actor: None,
                scope: String::new(),
                local,
                signed_ui,
                nonce,
                events: false,
                streams: BTreeMap::new(),
                next_binary_ref: 1,
                pair_request: None,
                web_host: false,
                attachments: AttachmentAssembler::new(
                    limits::PROMPT_ATTACHMENT_TOTAL_BYTES.max(limits::WEB_SCREENSHOT_BYTES),
                )?,
            },
        );
        Ok(id)
    }
    /// Forgets a closed connection and its waiters. The client is removed even when
    /// detaching its web host fails; that failure is returned for the log.
    pub fn disconnect(&mut self, id: &str) -> Result<()> {
        let web_result = self.disconnect_web_host(id);
        self.clients.remove(id);
        self.native_waiters.retain(|_, waiters| {
            waiters.retain(|waiter| waiter.connection != id);
            !waiters.is_empty()
        });
        self.language_waiters.retain(|_, waiters| {
            waiters.retain(|waiter| waiter.connection != id);
            !waiters.is_empty()
        });
        web_result
    }
    fn hello(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let hello: HelloParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid hello parameters"))?;
        let client = errors::invariant(
            self.clients.get(connection),
            "Handshake connection disappeared",
        )?;
        if client.actor.is_some() || client.pair_request.is_some() {
            return Err(errors::unauthorized("Handshake already completed"));
        }
        if hello.protocol_version != 1 {
            return Err(errors::protocol_version_unsupported(
                "Unsupported protocol version",
            ));
        }
        let web_host_claims = hello
            .capabilities
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|capability| **capability == ClientCapability::WebHost)
            .count();
        if web_host_claims > 1 {
            return Err(errors::invalid("Duplicate client capability"));
        }
        let wants_web_host = web_host_claims == 1;
        let local = client.local;
        if wants_web_host && !local {
            return Err(errors::unauthorized(
                "Web host capability requires a local human connection",
            ));
        }
        let nonce = client.nonce;
        let (role, device_id, pane_id, scope) = if let Some(binding) = hello.binding.as_deref() {
            if !local {
                return Err(errors::unauthorized(
                    "Agent bindings are accepted only on the local socket",
                ));
            }
            let (role, pane) = self.agent_identity(binding)?;
            (role, None, Some(pane.clone()), format!("agent:{pane}"))
        } else if local {
            // The listener has already verified the connecting operating-system uid.
            let uid = soyeht_platform::user::id()?;
            (
                Role::HumanLocal,
                None,
                None,
                format!("local:{uid}:{}", hello.client),
            )
        } else {
            let token = hello
                .device_token
                .as_deref()
                .ok_or_else(|| errors::unauthorized("Device token required"))?;
            let record = self
                .store
                .device_identity(&auth::token_hash(token))
                .map_err(errors::storage)?;
            let record = record.ok_or_else(|| errors::unauthorized("Unknown or revoked device"))?;
            let id = record.device_id;
            let key = auth::public_key(&STANDARD.encode(record.public_key))
                .map_err(|error| errors::unauthorized(error.to_string()))?;
            if !hello.proof.as_ref().is_some_and(|proof| {
                auth::hello_proof(&key, &nonce, &self.store.db_id, token, proof)
            }) {
                return Err(errors::unauthorized("Invalid connection signature"));
            }
            if self
                .clients
                .values()
                .filter(|c| c.actor.as_ref().and_then(|a| a.device_id.as_ref()) == Some(&id))
                .count()
                >= 8
            {
                return Err(errors::unauthorized("Device connection limit reached"));
            }
            (
                Role::HumanDevice,
                Some(id.clone()),
                None,
                format!("device:{id}"),
            )
        };
        let actor = Actor {
            role: role.clone(),
            client: hello.client,
            device_id,
            pane_id,
        };
        if wants_web_host {
            self.claim_web_host(connection, &actor.role)?;
        }
        let client = errors::invariant(
            self.clients.get_mut(connection),
            "Handshake connection disappeared",
        )?;
        client.scope = scope;
        client.actor = Some(actor);
        client.web_host = wants_web_host;
        serde_json::to_value(Welcome {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            protocol_version: 1,
            connection_id: connection.into(),
            role,
            capabilities: capabilities(),
            db_id: self.store.db_id.clone(),
        })
        .map_err(|_| errors::internal("Welcome response serialization failed"))
    }

    /// Only a local human connection may host the browser, and only one at a time.
    fn claim_web_host(&self, connection: &str, role: &Role) -> Result<()> {
        if *role != Role::HumanLocal {
            return Err(errors::unauthorized(
                "Web host capability requires a local human connection",
            ));
        }
        let taken = self
            .clients
            .iter()
            .any(|(id, client)| id != connection && client.web_host && !client.outbox.is_closed());
        if taken {
            return Err(errors::web_host_unavailable(
                "A web host is already connected",
            ));
        }
        Ok(())
    }

    fn agent_identity(&self, candidate: &str) -> Result<(Role, String)> {
        let mut found = None;
        for (pane, running) in &self.agents {
            let active = self
                .panes
                .get(pane)
                .is_some_and(|saved| !saved.stored.closed);
            if active && running.state.binding.matches(candidate) {
                if found.is_some() {
                    return Err(errors::internal("Agent binding is not unique"));
                }
                let role = match running.state.mcp_role {
                    Role::Agent => Role::Agent,
                    Role::Chief => Role::Chief,
                    _ => return Err(errors::internal("Agent checkpoint has an invalid role")),
                };
                found = Some((role, pane.clone()));
            }
        }
        found.ok_or_else(|| errors::unauthorized("Unknown agent binding"))
    }

    fn subscribe_events(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let since = params["since"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| errors::invalid("Invalid event cursor"))?;
        // Registration, the replay boundary, and publication all share the
        // Core mutex. No live event can enter between these steps.
        let (through, events) = self.store.replay(since).map_err(errors::storage)?;
        let client = errors::invariant(
            self.clients.get_mut(connection),
            "Subscription connection disappeared",
        )?;
        client.events = true;
        for event in events {
            client
                .outbox
                .send(model::notification(&event.method, event.params)?);
        }
        Ok(json!({"subscriptionId":Uuid::new_v4().to_string(),"through":through.to_string()}))
    }

    fn authenticated_scope(&self, connection: &str, method: &str) -> Result<String> {
        let client = self
            .clients
            .get(connection)
            .filter(|c| !c.outbox.is_closed())
            .ok_or_else(|| errors::unauthorized("Connection closed"))?;
        let actor = client
            .actor
            .as_ref()
            .ok_or_else(|| errors::unauthorized("Handshake required"))?;
        requires_signed_ui(method, client.signed_ui)?;
        if matches!(actor.role, Role::Agent | Role::Chief)
            && !method.starts_with("task.")
            && !method.starts_with("web.")
            && !matches!(method, method::AGENT_LIST | method::AGENT_MESSAGE)
            && !matches!(
                method,
                method::WRITING_STRUCTURE | method::WRITING_VARIANT_ADD
            )
            && method != method::LOOP_SET
        {
            return Err(errors::forbidden_role(
                "Agent binding method is not allowed",
            ));
        }
        // A paired device is a full remote control of this Mac (owner
        // decision 2026-09-26): once its hello is signed by an approved,
        // unrevoked key, it has the same powers as the local human.
        Ok(client.scope.clone())
    }

    fn repeated_result(
        &mut self,
        connection: &str,
        request_id: &Value,
        method: &str,
        params: &Value,
        scope: &str,
    ) -> Result<Option<Outcome>> {
        if matches!(method, method::TASK_DISPATCH | method::TASK_NEXT)
            || method == method::TASK_CREATE && params["dispatch"].as_bool() == Some(true)
        {
            return Ok(None);
        }
        // Look up before state-dependent validation: a repeated close still
        // succeeds after its pane has disappeared, without rerunning effects.
        if let Some(key) = params["idempotencyKey"].as_str() {
            let previous = self
                .store
                .idempotency_receipt(scope, key)
                .map_err(errors::storage)?;
            if let Some(previous) = previous {
                let incoming =
                    STANDARD.encode(auth::token_hash(&json!([method, params]).to_string()));
                if incoming != previous.fingerprint {
                    return Err(errors::idempotency_conflict(
                        "Idempotency key reused with different content",
                    ));
                }
                let value = previous.result;
                if uses_native_rpc_result(method) {
                    return self
                        .native_rpc_result(connection, request_id.clone(), method, params, value)
                        .map(Some);
                }
                return Ok(Some(Outcome::Reply(value)));
            }
        }
        Ok(None)
    }

    fn route(
        &mut self,
        connection: &str,
        request_id: Value,
        method: &str,
        params: &Value,
        prompt_attachments: Vec<crate::agents::attachments::PreparedAttachment>,
        writing_image: Option<crate::writing::attachments::WritingImageAttachment>,
    ) -> Result<Outcome> {
        let result = match method {
            method::STATE_SNAPSHOT => return self.snapshot_parts(connection),
            method::EVENTS_SUBSCRIBE => self.subscribe_events(connection, params)?,
            method::WORKSPACE_LIST => json!({"items":self.snapshot_section("workspaces")?}),
            method if crate::workspaces::handles(method) => {
                self.workspace_request(connection, method, params)?
            }
            method::PANE_CREATE => self.pane_create(connection, params)?,
            method::PANE_CLOSE
            | method::PANE_FOCUS
            | method::PANE_RENAME
            | method::PANE_RESIZE
            | method::PANE_RECORD_SET
            | method::PANE_EXPERIMENT_DELETE => self.pane_edit(connection, method, params)?,
            method::PANE_EXPERIMENT_SET => return self.pane_experiment_set(connection, params),
            method::PANE_EXPERIMENT_EXPORT => self.export_experiment(connection, params)?,
            method::PANE_EXPERIMENT_HOOK => return self.pane_experiment_hook(connection, params),
            method::PANE_INPUT => self.pane_input(connection, params)?,
            method::PANE_HARNESS_SWITCH => self.agent_harness_switch(connection, params)?,
            method::PANE_ROLE_SET => self.pane_role_set(connection, params)?,
            method::PANE_AUTH_START | method::PANE_AUTH_CANCEL | method::PANE_AUTH_LOGOUT => {
                self.agent_auth(connection, method, params)?
            }
            method if method.starts_with("harness.custom.") => {
                self.agent_custom_harness(connection, method, params)?
            }
            method if crate::agents::methods::handles(method) => {
                self.agent_method(connection, method, params, prompt_attachments)?
            }
            method::AGENT_LIST | method::AGENT_MESSAGE => {
                self.agent_peer_method(connection, method, params)?
            }
            method::PANE_PERMISSION_ANSWER => self.permission_answer(connection, method, params)?,
            method::PANE_ELICITATION_ANSWER => self.agent_answer(connection, method, params)?,
            method::TASK_SHOW | method::TASK_LIST | method::TASK_COMMENTS => {
                self.task_query(connection, method, params)?
            }
            method::PANE_COMMANDS_LIST => self.pane_commands_list(params)?,
            method if crate::skills::handles(method) => {
                self.skills_request(connection, method, params)?
            }
            method::USAGE_LIST => self.usage_list()?,
            method::USAGE_REFRESH => return self.usage_refresh(connection, params),
            method::SERVERS_LOCAL_LIST => return self.local_servers_list(),
            method::WEB_OPEN => self.web_open(connection, params)?,
            method::WEB_NAVIGATE => return self.web_navigate(connection, request_id, params),
            method::WEB_CLOSE => self.web_close(connection, params)?,
            method if crate::web::handles_command(method) => {
                return self.web_command(connection, request_id, method, params);
            }
            method if crate::web::handles_host_update(method) => {
                self.web_host_update(connection, method, params)?
            }
            method::LSP_STATUS => self.lsp_status(params)?,
            method::FILE_SYMBOLS | method::FILE_DEFINITION | method::FILE_REFERENCES => {
                return self.language_request(connection, request_id, method, params);
            }
            method::FILE_FORMAT => return self.language_format(connection, request_id, params),
            method::FILE_DIAGNOSTICS => self.file_diagnostics(params)?,
            method::FILE_DIFF => return self.file_diff(params),
            method::WRITING_ASSET_READ => return self.writing_asset_read(params),
            method::WRITING_SECTION_LIST => return self.writing_section_list(params),
            method::FILE_SEARCH => return self.file_search(connection, params),
            method::PROJECT_TREE_WATCH => return self.project_tree_watch(connection, params),
            method::PROJECT_ENTRY_RENAME | method::PROJECT_ENTRY_DELETE => {
                return self.project_entry_request(connection, method, params, false);
            }
            method if crate::editor::handles(method) || crate::writing::handles(method) => {
                self.document_request(connection, method, params, writing_image)?
            }
            method::TASK_DISPATCH => return self.task_dispatch(connection, params),
            method::TASK_NEXT => self.task_next(connection, params)?,
            method::TASK_WORKTREE_CLEAN => return self.task_worktree_clean(connection, params),
            method::LOOP_SET => self.loop_set(connection, params)?,
            method::TASK_REQUEST_REVIEW => return self.task_request_review(connection, params),
            method::TASK_CREATE
            | method::TASK_UPDATE
            | method::TASK_COMMENT
            | method::TASK_SUBITEM_ADD
            | method::TASK_SUBITEM_DONE
            | method::TASK_BLOCK
            | method::TASK_RELEASE
            | method::TASK_COMPLETE
            | method::TASK_UNBLOCK
            | method::TASK_MOVE => return self.task_author(connection, method, params),
            method::PANE_STREAM_SUBSCRIBE | method::PANE_STREAM_UNSUBSCRIBE => {
                return self.stream_request(connection, request_id, method, params);
            }
            method::ENGINE_STATUS => {
                json!({"version":env!("CARGO_PKG_VERSION"),"generation":self.generation,"ptyd":{"connected":self.ptyd.is_some(),"version":soyeht_ptyd::wire::VERSION,"generation":self.ptyd.as_ref().map_or(0,|p|p.supervisor_generation())},"db":{"id":self.store.db_id,"schemaVersion":soyeht_store::database::SCHEMA_VERSION},"reachability":self.reachability})
            }
            method::SETTINGS_GET => {
                let key = errors::param_str(params, "key")?;
                json!({"key":params["key"],"value":self.setting(key)?})
            }
            method::SETTINGS_SET => self.setting_set(connection, params)?,
            method::DEVICE_LIST => json!({"items":self.snapshot_section("devices")?}),
            method::DEVICE_PAIR_START | method::DEVICE_PAIR_DECIDE | method::DEVICE_REVOKE => {
                self.device_request(connection, method, params)?
            }
            method::PUSH_REGISTER | method::PUSH_UNREGISTER => {
                self.push_request(connection, method, params)?
            }
            _ => return Err(errors::invalid("Method unavailable")),
        };
        if uses_native_rpc_result(method) {
            return self.native_rpc_result(connection, request_id, method, params, result);
        }
        Ok(Outcome::Reply(result))
    }

    fn permission_answer(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let request_id = errors::param_str(params, "requestId")?;
        if StorePort::web_permission_request(&self.store, request_id)
            .map_err(errors::storage)?
            .is_some()
        {
            self.web_permission_answer(connection, params)
        } else {
            self.agent_answer(connection, method, params)
        }
    }

    fn snapshot_section(&mut self, section: &str) -> Result<Value> {
        self.store
            .snapshot()
            .map_err(errors::storage)?
            .sections
            .into_iter()
            .find(|(name, _)| name == section)
            .map(|(_, value)| value)
            .ok_or_else(|| errors::internal(format!("Snapshot has no {section} section")))
    }

    /// `dispatch`, with any offloaded work run on this thread.
    #[cfg(test)]
    pub(crate) fn dispatch_inline(&mut self, connection: &str, request: Value) -> Result<Outcome> {
        match self.dispatch(connection, request)? {
            Outcome::Offload(offload) => offload.run_inline(self),
            outcome => Ok(outcome),
        }
    }

    pub fn dispatch(&mut self, connection: &str, request: Value) -> Result<Outcome> {
        if request["jsonrpc"] != "2.0" || !request["id"].is_string() && !request["id"].is_u64() {
            return Err(errors::invalid("Expected JSON-RPC 2.0 request with id"));
        }
        let method = request["method"]
            .as_str()
            .ok_or_else(|| errors::invalid("Missing method"))?;
        let mut params = request.get("params").cloned().unwrap_or(json!({}));
        soyeht_protocol::validate::parameters(method, &params)
            .map_err(|error| errors::invalid(error.to_string()))?;
        if method == method::HELLO {
            return self
                .hello(connection, &params)
                .map(Outcome::Reply)
                .inspect_err(|error| log_refused_handshake(connection, &params, error));
        }
        if method.starts_with("pair.") {
            return self
                .pair_request(connection, method, &params)
                .map(Outcome::Reply);
        }
        let scope = self.authenticated_scope(connection, method)?;
        let prompt_attachments = if method == method::PANE_PROMPT {
            self.prepare_prompt_attachments(connection, &mut params)?
        } else {
            Vec::new()
        };
        let writing_image = if method == method::WRITING_FORMAT {
            let image = self.prepare_writing_image(connection, &mut params)?;
            if image.is_none() && self.web_attachment_pending(connection)? {
                return Err(errors::invalid(
                    "Binary attachments must be committed by the following request",
                ));
            }
            image
        } else {
            if self.web_attachment_pending(connection)? {
                return Err(errors::invalid(
                    "Binary attachments must be committed by the following request",
                ));
            }
            None
        };
        if let Some(outcome) =
            self.repeated_result(connection, &request["id"], method, &params, &scope)?
        {
            return Ok(outcome);
        }
        if matches!(method, method::HARNESS_LIST | method::MODEL_LIST) {
            return Ok(Outcome::Catalog {
                method: method.to_owned(),
                params,
            });
        }
        self.route(
            connection,
            request["id"].clone(),
            method,
            &params,
            prompt_attachments,
            writing_image,
        )
    }

    fn snapshot_parts(&mut self, connection: &str) -> Result<Outcome> {
        let snapshot = self.store.snapshot().map_err(errors::storage)?;
        let mut sections = snapshot.sections;
        let pairing = self.pending_pair_view()?;
        if let Some((_, requests)) = sections.iter_mut().find(|(name, _)| name == "requests") {
            let requests = requests
                .as_array_mut()
                .ok_or_else(|| errors::internal("Snapshot requests section is not an array"))?;
            let pairing = pairing
                .as_array()
                .ok_or_else(|| errors::internal("Pending pairing snapshot is not an array"))?;
            requests.extend(pairing.iter().cloned());
        } else {
            sections.push(("requests".into(), pairing));
        }
        sections.push(("capabilities".into(), json!(capabilities())));
        sections.push(("writing".into(), self.writing_snapshot()?));
        let streams:Vec<_>=self.panes.values().filter(|p|!p.stored.closed && p.stored.pane.mode == soyeht_protocol::PaneMode::Terminal).map(|p|json!({"paneId":p.stored.pane.pane_id,"streamId":p.stored.pane.stream_id,"generation":p.stored.pane.generation,"firstRetainedOffset":p.screen.first_offset().to_string(),"nextOffset":p.screen.next_offset().to_string(),"binaryPaneRef":0})).collect();
        sections.push(("streams".into(), json!(streams)));
        let snapshot_id = Uuid::new_v4().to_string();
        let mut pages = vec![];
        for (name, section) in sections {
            let mut page = vec![];
            let mut size = 0;
            let items = section.as_array().ok_or_else(|| {
                errors::internal(format!("Snapshot section {name} is not an array"))
            })?;
            for item in items {
                let bytes = serde_json::to_vec(item)
                    .map_err(|_| {
                        errors::internal(format!("Snapshot section {name} cannot be encoded"))
                    })?
                    .len()
                    + 1;
                if bytes > soyeht_protocol::limits::JSON_BYTES - 512 {
                    return Err(errors::snapshot_too_large(
                        "Snapshot item exceeds frame capacity",
                    ));
                }
                if size + bytes > soyeht_protocol::limits::JSON_BYTES - 512 {
                    pages.push((name.clone(), std::mem::take(&mut page)));
                    size = 0;
                }
                page.push(item.clone());
                size += bytes;
            }
            pages.push((name, page));
        }
        let count = pages.len();
        let scope = errors::invariant(
            self.clients.get(connection),
            "Snapshot connection disappeared",
        )?
        .scope
        .clone();
        let focus = self.store.client_focus(&scope).map_err(errors::storage)?;
        let mut focus_value = json!({});
        if let Some(focus) = focus {
            if let Some(workspace) = focus.workspace_id {
                focus_value["workspaceId"] = json!(workspace);
            }
            if let Some(pane) = focus.pane_id {
                focus_value["paneId"] = json!(pane);
            }
        }
        let parts = pages
            .into_iter()
            .enumerate()
            .map(|(index, (section, data))| {
                model::notification(
                    event::STATE_SNAPSHOT_PART,
                    json!({"snapshotId":snapshot_id,"index":index,"of":count,"section":section,"data":data}),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Outcome::Snapshot {
            parts,
            result: json!({"snapshotId":snapshot_id,"cursor":snapshot.cursor.to_string(),"parts":count,"focus":focus_value}),
        })
    }
}

pub fn capabilities() -> Vec<String> {
    vec![
        "terminal".into(),
        "workspaces".into(),
        "snapshot-parts".into(),
        "event-replay".into(),
        "terminal-replay".into(),
        "device-pairing".into(),
        "workspace-tasks".into(),
        "subscription-usage".into(),
    ]
}

#[cfg(test)]
mod experiment_consent_tests {
    use super::*;

    #[test]
    fn capture_mutation_requires_the_attested_ui_program() {
        assert!(requires_signed_ui("pane.experiment.set", true).is_ok());
        assert!(requires_signed_ui("pane.experiment.delete", true).is_ok());
        assert!(requires_signed_ui("pane.experiment.export", true).is_ok());
        assert_eq!(
            errors::kind(&requires_signed_ui("pane.experiment.set", false).unwrap_err()),
            soyeht_protocol::ErrorKind::ForbiddenRole
        );
        assert_eq!(
            errors::kind(&requires_signed_ui("pane.experiment.delete", false).unwrap_err()),
            soyeht_protocol::ErrorKind::ForbiddenRole
        );
        assert_eq!(
            errors::kind(&requires_signed_ui("pane.experiment.export", false).unwrap_err()),
            soyeht_protocol::ErrorKind::ForbiddenRole
        );
        assert!(requires_signed_ui("pane.focus", false).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(line: &Line) -> Value {
        serde_json::from_str(&line.json()).unwrap()
    }

    #[test]
    fn a_refused_handshake_names_a_plain_client_and_never_a_credential() {
        let params = json!({
            "client": "soyeht-mcp",
            "binding": "secret-binding",
            "deviceToken": "secret-token",
            "proof": "secret-proof",
        });
        let record = object(&refused_handshake_record(
            "connection",
            &params,
            &errors::unauthorized("Unknown agent binding"),
        ));
        assert_eq!(record["client"], "soyeht-mcp");
        assert_eq!(record["kind"], "unauthorized");
        assert_eq!(record["reason"], "Unknown agent binding");
        assert!(!record.to_string().contains("secret"));
    }

    #[test]
    fn a_client_label_that_is_not_plain_is_not_logged() {
        for label in ["", "has space", "path/like", "x".repeat(33).as_str()] {
            let record = object(&refused_handshake_record(
                "connection",
                &json!({"client": label}),
                &errors::unauthorized("Unknown or revoked device"),
            ));
            assert!(record["client"].is_null(), "{label}");
        }
    }
}
