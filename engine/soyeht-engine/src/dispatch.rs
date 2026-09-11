use crate::{
    auth,
    core::Core,
    errors::{self, Result},
    model::{self, Connected},
    outbox::Outbox,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::{Actor, ErrorKind, HelloParams, Role, Welcome, frame::Frame};
use std::collections::BTreeMap;
use uuid::Uuid;

pub enum Outcome {
    Reply(Value),
    Queued,
    Snapshot { parts: Vec<Frame>, result: Value },
}

impl Core {
    pub fn connect(&mut self, local: bool, outbox: Outbox) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let nonce = auth::random_bytes().map_err(|s| errors::error(ErrorKind::InternalError, s))?;
        outbox.send(model::notification(
            "auth.challenge",
            json!({"nonce":STANDARD.encode(nonce),"engineId":self.store.db_id}),
        ));
        self.clients.insert(
            id.clone(),
            Connected {
                outbox,
                actor: None,
                scope: String::new(),
                local,
                nonce,
                events: false,
                streams: BTreeMap::new(),
                next_binary_ref: 1,
                pair_request: None,
            },
        );
        Ok(id)
    }
    fn hello(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let hello: HelloParams = serde_json::from_value(params.clone()).expect("validated hello");
        let client = &self.clients[connection];
        if client.actor.is_some() || client.pair_request.is_some() {
            return Err(errors::error(
                ErrorKind::Unauthorized,
                "Handshake already completed",
            ));
        }
        if hello.protocol_version != 1 {
            return Err(errors::error(
                ErrorKind::ProtocolVersionUnsupported,
                "Unsupported protocol version",
            ));
        }
        if hello.binding.is_some() {
            return Err(errors::error(
                ErrorKind::Unauthorized,
                "This engine has not issued an agent binding",
            ));
        }
        let (role, device_id, scope) = if client.local {
            // The listener has already verified the connecting operating-system uid.
            // SAFETY: geteuid has no arguments or memory preconditions.
            let uid = unsafe { libc::geteuid() };
            (
                Role::HumanLocal,
                None,
                format!("local:{uid}:{}", hello.client),
            )
        } else {
            let token = hello
                .device_token
                .as_deref()
                .ok_or_else(|| errors::error(ErrorKind::Unauthorized, "Device token required"))?;
            let record = self
                .store
                .device_identity(&auth::token_hash(token))
                .map_err(errors::storage)?;
            let record = record.ok_or_else(|| {
                errors::error(ErrorKind::Unauthorized, "Unknown or revoked device")
            })?;
            let id = record.device_id;
            let key = auth::public_key(&STANDARD.encode(record.public_key))
                .map_err(|s| errors::error(ErrorKind::Unauthorized, s))?;
            if !hello.proof.as_ref().is_some_and(|proof| {
                auth::hello_proof(&key, &client.nonce, &self.store.db_id, token, proof)
            }) {
                return Err(errors::error(
                    ErrorKind::Unauthorized,
                    "Invalid connection signature",
                ));
            }
            if self
                .clients
                .values()
                .filter(|c| c.actor.as_ref().and_then(|a| a.device_id.as_ref()) == Some(&id))
                .count()
                >= 8
            {
                return Err(errors::error(
                    ErrorKind::Unauthorized,
                    "Device connection limit reached",
                ));
            }
            (Role::HumanDevice, Some(id.clone()), format!("device:{id}"))
        };
        let actor = Actor {
            role: role.clone(),
            client: hello.client,
            device_id,
            pane_id: None,
        };
        let client = self.clients.get_mut(connection).expect("client");
        client.scope = scope;
        client.actor = Some(actor);
        Ok(serde_json::to_value(Welcome {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            protocol_version: 1,
            connection_id: connection.into(),
            role,
            capabilities: capabilities(),
            db_id: self.store.db_id.clone(),
        })
        .expect("welcome"))
    }

    pub fn dispatch(&mut self, connection: &str, request: Value) -> Result<Outcome> {
        if request["jsonrpc"] != "2.0" || !request["id"].is_string() && !request["id"].is_u64() {
            return Err(errors::error(
                ErrorKind::InvalidParams,
                "Expected JSON-RPC 2.0 request with id",
            ));
        }
        let method = request["method"]
            .as_str()
            .ok_or_else(|| errors::error(ErrorKind::InvalidParams, "Missing method"))?;
        let params = request.get("params").cloned().unwrap_or(json!({}));
        soyeht_protocol::validate::parameters(method, &params)
            .map_err(|s| errors::error(ErrorKind::InvalidParams, s))?;
        if method == "hello" {
            return self.hello(connection, &params).map(Outcome::Reply);
        }
        if method.starts_with("pair.") {
            return self
                .pair_request(connection, method, &params)
                .map(Outcome::Reply);
        }
        let client = self
            .clients
            .get(connection)
            .filter(|c| !c.outbox.is_closed())
            .ok_or_else(|| errors::error(ErrorKind::Unauthorized, "Connection closed"))?;
        let actor = client
            .actor
            .as_ref()
            .ok_or_else(|| errors::error(ErrorKind::Unauthorized, "Handshake required"))?;
        if actor.role == Role::HumanDevice
            && (method == "pane.input"
                || method == "settings.set"
                || method.starts_with("device.") && method != "device.list")
        {
            return Err(errors::error(
                ErrorKind::ForbiddenRole,
                "Operation requires the local human client",
            ));
        }
        // Look up before state-dependent validation: a repeated close still
        // succeeds after its pane has disappeared, without rerunning effects.
        if let Some(key) = params["idempotencyKey"].as_str() {
            let previous = self
                .store
                .idempotency_receipt(&client.scope, key)
                .map_err(errors::storage)?;
            if let Some(previous) = previous {
                let incoming =
                    STANDARD.encode(auth::token_hash(&json!([method, params]).to_string()));
                if incoming != previous.fingerprint {
                    return Err(errors::error(
                        ErrorKind::IdempotencyConflict,
                        "Idempotency key reused with different content",
                    ));
                }
                let value = previous.result;
                if matches!(method, "pane.config.set" | "pane.yolo") {
                    return self.native_rpc_result(connection, request["id"].clone(), key, value);
                }
                return Ok(Outcome::Reply(value));
            }
        }
        let result = match method {
            "state.snapshot" => return self.snapshot_parts(connection),
            "events.subscribe" => {
                let since = params["since"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .expect("validated cursor");
                // Registration, the replay boundary, and publication all share
                // the Core mutex. No live event can enter between these steps.
                let (through, events) = self.store.replay(since).map_err(errors::storage)?;
                let client = self.clients.get_mut(connection).expect("client");
                client.events = true;
                for event in events {
                    client
                        .outbox
                        .send(model::notification(&event.method, event.params));
                }
                json!({"subscriptionId":Uuid::new_v4().to_string(),"through":through.to_string()})
            }
            "workspace.list" => {
                let snapshot = self.store.snapshot().map_err(errors::storage)?;
                json!({"items":snapshot.sections.into_iter().find(|(name,_)|name=="workspaces").expect("workspaces").1})
            }
            "workspace.create" => self.workspace_create(connection, &params)?,
            "workspace.rename" | "workspace.close" | "workspace.focus" => {
                self.workspace_edit(connection, method, &params)?
            }
            "pane.create" => self.pane_create(connection, &params)?,
            "pane.close" | "pane.focus" | "pane.rename" | "pane.resize" | "pane.record.set" => {
                self.pane_edit(connection, method, &params)?
            }
            "pane.input" => self.pane_input(connection, &params)?,
            "pane.yolo" => self.agent_yolo(connection, &params)?,
            "pane.auth.start" => self.agent_auth_start(connection, &params)?,
            "harness.list" | "model.list" => self.agent_catalog(method, &params)?,
            "pane.prompt" | "pane.queue.list" | "pane.queue.remove" | "pane.queue.resume"
            | "pane.cancel" | "pane.config.set" | "pane.transcript" | "pane.draft" => {
                self.agent_method(connection, method, &params)?
            }
            "pane.permission.answer" | "pane.elicitation.answer" => {
                self.agent_answer(connection, method, &params)?
            }
            "pane.stream.subscribe" => {
                self.subscribe_stream(connection, request["id"].clone(), &params)?;
                return Ok(Outcome::Queued);
            }
            "pane.stream.unsubscribe" => {
                self.clients
                    .get_mut(connection)
                    .expect("client")
                    .streams
                    .remove(params["paneId"].as_str().expect("pane"));
                json!({})
            }
            "engine.status" => {
                json!({"version":env!("CARGO_PKG_VERSION"),"generation":self.generation,"ptyd":{"connected":self.ptyd.is_some(),"version":soyeht_ptyd::wire::VERSION,"generation":self.ptyd.as_ref().map_or(0,|p|p.supervisor_generation)},"db":{"id":self.store.db_id,"schemaVersion":soyeht_store::database::SCHEMA_VERSION}})
            }
            "settings.get" => {
                json!({"key":params["key"],"value":self.setting(params["key"].as_str().expect("key"))?})
            }
            "settings.set" => self.setting_set(connection, &params)?,
            "device.list" => {
                let snapshot = self.store.snapshot().map_err(errors::storage)?;
                json!({"items":snapshot.sections.into_iter().find(|(name,_)|name=="devices").expect("devices").1})
            }
            "device.pair.start" | "device.pair.decide" | "device.revoke" => {
                self.device_request(connection, method, &params)?
            }
            _ => {
                return Err(errors::error(
                    ErrorKind::InvalidParams,
                    "Method unavailable",
                ));
            }
        };
        if matches!(method, "pane.config.set" | "pane.yolo") {
            return self.native_rpc_result(
                connection,
                request["id"].clone(),
                params["idempotencyKey"].as_str().expect("validated key"),
                result,
            );
        }
        Ok(Outcome::Reply(result))
    }

    fn snapshot_parts(&mut self, connection: &str) -> Result<Outcome> {
        let snapshot = self.store.snapshot().map_err(errors::storage)?;
        let mut sections = snapshot.sections;
        sections.push(("tasks".into(), json!([])));
        let pairing = self.pending_pair_view()?;
        if let Some((_, requests)) = sections.iter_mut().find(|(name, _)| name == "requests") {
            let requests = requests.as_array_mut().ok_or_else(|| {
                errors::error(
                    ErrorKind::InternalError,
                    "Snapshot requests section is not an array",
                )
            })?;
            let pairing = pairing.as_array().ok_or_else(|| {
                errors::error(
                    ErrorKind::InternalError,
                    "Pending pairing snapshot is not an array",
                )
            })?;
            requests.extend(pairing.iter().cloned());
        } else {
            sections.push(("requests".into(), pairing));
        }
        sections.push(("capabilities".into(), json!(capabilities())));
        let streams:Vec<_>=self.panes.values().filter(|p|!p.stored.closed && p.stored.pane.mode == soyeht_protocol::PaneMode::Terminal).map(|p|json!({"paneId":p.stored.pane.pane_id,"streamId":p.stored.pane.stream_id,"generation":p.stored.pane.generation,"firstRetainedOffset":p.screen.first_offset().to_string(),"nextOffset":p.screen.next_offset().to_string(),"binaryPaneRef":0})).collect();
        sections.push(("streams".into(), json!(streams)));
        let snapshot_id = Uuid::new_v4().to_string();
        let mut pages = vec![];
        for (name, section) in sections {
            let mut page = vec![];
            let mut size = 0;
            let items = section.as_array().ok_or_else(|| {
                errors::error(
                    ErrorKind::InternalError,
                    format!("Snapshot section {name} is not an array"),
                )
            })?;
            for item in items {
                let bytes = serde_json::to_vec(item)
                    .map_err(|_| {
                        errors::error(
                            ErrorKind::InternalError,
                            format!("Snapshot section {name} cannot be encoded"),
                        )
                    })?
                    .len()
                    + 1;
                if bytes > soyeht_protocol::limits::JSON_BYTES - 512 {
                    return Err(errors::error(
                        ErrorKind::SnapshotTooLarge,
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
        let focus = self
            .store
            .client_focus(&self.clients[connection].scope)
            .map_err(errors::storage)?;
        let mut focus_value = json!({});
        if let Some(focus) = focus {
            if let Some(workspace) = focus.workspace_id {
                focus_value["workspaceId"] = json!(workspace);
            }
            if let Some(pane) = focus.pane_id {
                focus_value["paneId"] = json!(pane);
            }
        }
        let parts=pages.into_iter().enumerate().map(|(index,(section,data))|model::notification("state.snapshot_part",json!({"snapshotId":snapshot_id,"index":index,"of":count,"section":section,"data":data}))).collect();
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
    ]
}
