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
use soyeht_protocol::{
    Actor, AttachmentRef, ClientCapability, HelloParams, Role, Welcome,
    attachment::AttachmentAssembler, frame::Frame, limits,
};
use std::collections::BTreeMap;
use uuid::Uuid;

pub enum Outcome {
    Reply(Value),
    Catalog {
        method: String,
        params: Value,
    },
    WritingSectionList(crate::writing::PreparedWritingSectionList),
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
        "pane.experiment.set" | "pane.experiment.delete" | "pane.experiment.export"
    ) && !signed_ui
    {
        return Err(errors::forbidden_role(
            "Pane-state experiment data can be recorded, deleted, or exported only in the signed Soyeht app on this Mac",
        ));
    }
    Ok(())
}

fn uses_native_rpc_result(method: &str) -> bool {
    matches!(
        method,
        "pane.config.set" | "pane.yolo" | "pane.harness.switch"
    )
}

impl Core {
    pub fn connect(&mut self, local: bool, signed_ui: bool, outbox: Outbox) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let nonce = auth::random_bytes().map_err(|error| errors::internal(error.to_string()))?;
        outbox.send(model::notification(
            "auth.challenge",
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
                attachments: AttachmentAssembler::new(limits::WEB_SCREENSHOT_BYTES)?,
            },
        );
        Ok(id)
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
            // SAFETY: geteuid has no arguments or memory preconditions.
            let uid = unsafe { libc::geteuid() };
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
            if actor.role != Role::HumanLocal {
                return Err(errors::unauthorized(
                    "Web host capability requires a local human connection",
                ));
            }
            if self.clients.iter().any(|(id, client)| {
                id != connection && client.web_host && !client.outbox.is_closed()
            }) {
                return Err(errors::unauthorized("A web host is already connected"));
            }
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
            && !matches!(method, "agent.list" | "agent.message")
            && !matches!(method, "writing.structure" | "writing.variant.add")
            && method != "loop.set"
        {
            return Err(errors::forbidden_role(
                "Agent binding method is not allowed",
            ));
        }
        if actor.role == Role::HumanDevice
            && (method == "pane.input"
                || method == "settings.set"
                || method.starts_with("harness.custom.")
                || method.starts_with("device.") && method != "device.list")
        {
            return Err(errors::forbidden_role(
                "Operation requires the local human client",
            ));
        }
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
        if matches!(method, "task.dispatch" | "task.next")
            || method == "task.create" && params["dispatch"].as_bool() == Some(true)
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
                if matches!(
                    method,
                    "pane.config.set" | "pane.yolo" | "pane.harness.switch"
                ) {
                    return self
                        .native_rpc_result(connection, request_id.clone(), key, value)
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
        prompt_attachments: Vec<soyeht_store::agents::PromptAttachment>,
        writing_image: Option<crate::writing::attachments::WritingImageAttachment>,
    ) -> Result<Outcome> {
        let result = match method {
            "state.snapshot" => return self.snapshot_parts(connection),
            "events.subscribe" => self.subscribe_events(connection, params)?,
            "workspace.list" => json!({"items":self.snapshot_section("workspaces")?}),
            method if crate::workspaces::handles(method) => {
                self.workspace_request(connection, method, params)?
            }
            "pane.create" => self.pane_create(connection, params)?,
            "pane.close"
            | "pane.focus"
            | "pane.rename"
            | "pane.resize"
            | "pane.record.set"
            | "pane.experiment.set"
            | "pane.experiment.delete" => self.pane_edit(connection, method, params)?,
            "pane.experiment.export" => self.export_experiment(connection, params)?,
            "pane.experiment.hook" => self.record_hook_event(connection, params)?,
            "pane.input" => self.pane_input(connection, params)?,
            "pane.yolo" => self.agent_yolo(connection, params)?,
            "pane.harness.switch" => self.agent_harness_switch(connection, params)?,
            "pane.role.set" => self.pane_role_set(connection, params)?,
            "pane.auth.start" => self.agent_auth_start(connection, params)?,
            method if method.starts_with("harness.custom.") => {
                self.agent_custom_harness(connection, method, params)?
            }
            method if crate::agents::methods::handles(method) => {
                self.agent_method(connection, method, params, prompt_attachments)?
            }
            "agent.list" | "agent.message" => self.agent_peer_method(connection, method, params)?,
            "pane.permission.answer" => self.permission_answer(connection, method, params)?,
            "pane.elicitation.answer" => self.agent_answer(connection, method, params)?,
            "task.show" | "task.list" | "task.comments" => {
                self.task_query(connection, method, params)?
            }
            "usage.list" => self.usage_list()?,
            "usage.refresh" => self.usage_refresh(connection, params)?,
            "servers.local.list" => self.local_servers_list()?,
            "web.open" => self.web_open(connection, params)?,
            "web.navigate" => return self.web_navigate(connection, request_id, params),
            "web.close" => self.web_close(connection, params)?,
            method if crate::web::handles_command(method) => {
                return self.web_command(connection, request_id, method, params);
            }
            method if crate::web::handles_host_update(method) => {
                self.web_host_update(connection, method, params)?
            }
            "lsp.status" => self.lsp_status(params)?,
            "file.symbols" | "file.definition" | "file.references" => {
                return self.language_request(connection, request_id, method, params);
            }
            "file.format" => return self.language_format(connection, request_id, params),
            "file.diagnostics" | "file.diff" => self.file_inspection(method, params)?,
            "writing.asset.read" => return self.writing_asset_read(params),
            "writing.section.list" => {
                return self
                    .prepare_writing_section_list(params)
                    .map(Outcome::WritingSectionList);
            }
            method if crate::editor::handles(method) || crate::writing::handles(method) => {
                self.document_request(connection, method, params, writing_image)?
            }
            "task.dispatch" => self.task_dispatch(connection, params)?,
            "task.next" => self.task_next(connection, params)?,
            "task.worktree.clean" => self.task_worktree_clean(connection, params)?,
            "loop.set" => self.loop_set(connection, params)?,
            "task.create"
            | "task.update"
            | "task.comment"
            | "task.subitem.add"
            | "task.subitem.done"
            | "task.block"
            | "task.release"
            | "task.request_review"
            | "task.complete"
            | "task.unblock"
            | "task.move" => self.task_author(connection, method, params)?,
            "pane.stream.subscribe" => {
                self.subscribe_stream(connection, request_id, params)?;
                return Ok(Outcome::Queued);
            }
            "pane.stream.unsubscribe" => {
                let pane_id = errors::param_str(params, "paneId")?;
                errors::invariant(
                    self.clients.get_mut(connection),
                    "Stream connection disappeared",
                )?
                .streams
                .remove(pane_id);
                json!({})
            }
            "engine.status" => {
                json!({"version":env!("CARGO_PKG_VERSION"),"generation":self.generation,"ptyd":{"connected":self.ptyd.is_some(),"version":soyeht_ptyd::wire::VERSION,"generation":self.ptyd.as_ref().map_or(0,|p|p.supervisor_generation())},"db":{"id":self.store.db_id,"schemaVersion":soyeht_store::database::SCHEMA_VERSION}})
            }
            "settings.get" => {
                let key = errors::param_str(params, "key")?;
                json!({"key":params["key"],"value":self.setting(key)?})
            }
            "settings.set" => self.setting_set(connection, params)?,
            "device.list" => json!({"items":self.snapshot_section("devices")?}),
            "device.pair.start" | "device.pair.decide" | "device.revoke" => {
                self.device_request(connection, method, params)?
            }
            _ => return Err(errors::invalid("Method unavailable")),
        };
        if uses_native_rpc_result(method) {
            return self.native_rpc_result(
                connection,
                request_id,
                errors::param_str(params, "idempotencyKey")?,
                result,
            );
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
        if method == "hello" {
            return self.hello(connection, &params).map(Outcome::Reply);
        }
        if method.starts_with("pair.") {
            return self
                .pair_request(connection, method, &params)
                .map(Outcome::Reply);
        }
        let scope = self.authenticated_scope(connection, method)?;
        let prompt_attachments = if method == "pane.prompt" {
            self.prepare_prompt_attachments(connection, &mut params)?
        } else {
            Vec::new()
        };
        let writing_image = if method == "writing.format" {
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
        if matches!(method, "harness.list" | "model.list") {
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
                    "state.snapshot_part",
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
