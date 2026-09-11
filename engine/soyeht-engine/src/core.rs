use crate::{
    errors::{self, Result},
    model::{self, Connected, RunningPane, StoredPane},
    screen::Screen,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::{ErrorKind, PaneState, StateSource};
use soyeht_ptyd::{
    client::Client,
    wire::{Instance, ProcessKind, Request, Response, Spawn},
};
use soyeht_store::{
    database::{Change, Event, Mutation, Store},
    segments::Record,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub struct Core {
    pub store: Store,
    pub directory: PathBuf,
    pub generation: u64,
    pub ptyd: Option<Client>,
    pub panes: BTreeMap<String, RunningPane>,
    pub agents: BTreeMap<String, crate::agents::Running>,
    pub native_waiters: BTreeMap<(String, String), Vec<(String, Value)>>,
    pub clients: BTreeMap<String, Connected>,
    pub pair_address: String,
}

impl Core {
    pub fn open(directory: &Path) -> Result<Self> {
        let mut store = Store::open(directory).map_err(errors::storage)?;
        let generation = store.next_engine_generation().map_err(errors::storage)?;
        if generation > u32::MAX as u64 {
            return Err(errors::error(
                ErrorKind::InternalError,
                "Engine generation exhausted",
            ));
        }
        let mut core = Self {
            store,
            directory: directory.into(),
            generation,
            ptyd: None,
            panes: BTreeMap::new(),
            agents: BTreeMap::new(),
            native_waiters: BTreeMap::new(),
            clients: BTreeMap::new(),
            pair_address: "127.0.0.1:8291".into(),
        };
        for stored in model::panes(&core.store)? {
            if stored.pane.mode == soyeht_protocol::PaneMode::Agent {
                let mut state: crate::agents::state::State =
                    soyeht_store::agents::runtime(&core.store, &stored.pane.pane_id)
                        .map_err(errors::storage)?
                        .ok_or_else(|| {
                            errors::error(ErrorKind::InternalError, "Agent pane has no checkpoint")
                        })?;
                state.queue = soyeht_store::agents::queue(&core.store, &stored.pane.pane_id)
                    .map_err(errors::storage)?
                    .into();
                core.agents.insert(
                    stored.pane.pane_id.clone(),
                    crate::agents::Running::new(state),
                );
            }
            let screen = Screen::open(
                &directory.join("sessions").join(&stored.pane.pane_id),
                stored.pane.record,
            )
            .map_err(errors::io)?;
            core.panes
                .insert(stored.pane.pane_id.clone(), RunningPane { stored, screen });
        }
        // The supervisor can start after the engine under the service manager.
        // No process is declared dead until its registry has been reconciled.
        let _ = core.connect_supervisor();
        core.system_event("engine.restarted", json!({"generation":generation}))?;
        Ok(core)
    }

    fn system_event(&mut self, method: &str, params: Value) -> Result<()> {
        let mutation = self
            .store
            .mutate("engine", &Uuid::new_v4().to_string(), method, |_| {
                Ok((json!({}), vec![(method.into(), params)]))
            })
            .map_err(errors::storage)?;
        self.publish(&mutation.events);
        Ok(())
    }

    pub fn publish(&self, events: &[Event]) {
        for client in self
            .clients
            .values()
            .filter(|c| c.events && c.actor.is_some())
        {
            for event in events {
                client
                    .outbox
                    .send(model::notification(&event.method, event.params.clone()));
            }
        }
    }

    pub fn mutate<F>(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
        change: F,
    ) -> Result<Mutation>
    where
        F: FnOnce(&Change<'_>) -> soyeht_store::database::Result<(Value, Vec<(String, Value)>)>,
    {
        let client = self
            .clients
            .get(connection)
            .ok_or_else(|| errors::error(ErrorKind::Unauthorized, "Connection not found"))?;
        let actor = serde_json::to_value(
            client
                .actor
                .as_ref()
                .ok_or_else(|| errors::error(ErrorKind::Unauthorized, "Handshake required"))?,
        )
        .expect("actor");
        let key = params["idempotencyKey"]
            .as_str()
            .ok_or_else(|| errors::error(ErrorKind::InvalidParams, "Missing idempotency key"))?;
        let fingerprint = STANDARD.encode(crate::auth::token_hash(
            &json!([method, params]).to_string(),
        ));
        let mutation = self
            .store
            .mutate_as(&client.scope, key, &fingerprint, Some(&actor), change)
            .map_err(errors::storage)?;
        self.publish(&mutation.events);
        Ok(mutation)
    }

    pub fn pty_request(&mut self, request: Request) -> Result<Response> {
        let result = self
            .ptyd
            .as_mut()
            .ok_or_else(|| errors::error(ErrorKind::PtydUnavailable, "Supervisor disconnected"))?
            .request(request);
        match result {
            Ok(Response::Error { kind, message }) => Err(errors::error(
                if kind == "stale_engine_generation" {
                    ErrorKind::StaleEngineGeneration
                } else if kind == "spawn_failed" {
                    ErrorKind::HarnessUnavailable
                } else {
                    ErrorKind::PtydUnavailable
                },
                &message,
            )),
            Ok(response) => Ok(response),
            Err(_) => {
                self.ptyd = None;
                let _ = self.system_event(
                    "ptyd.lost",
                    json!({"reason":"Supervisor connection closed"}),
                );
                Err(errors::error(
                    ErrorKind::PtydUnavailable,
                    "Supervisor disconnected",
                ))
            }
        }
    }

    pub fn connect_supervisor(&mut self) -> Result<()> {
        let client = Client::connect(&self.directory.join("ptyd.sock"), self.generation)
            .map_err(|_| errors::error(ErrorKind::PtydUnavailable, "Supervisor unavailable"))?;
        self.ptyd = Some(client);
        let result = self.reconcile_registry();
        if result.is_err() {
            self.ptyd = None;
        } else {
            self.system_event("ptyd.reconnected",json!({"generation":self.ptyd.as_ref().expect("connected supervisor").supervisor_generation}))?;
        }
        result
    }

    fn reconcile_registry(&mut self) -> Result<()> {
        let Response::InputReceipts(receipts) = self.pty_request(Request::PendingInputs)? else {
            return Err(errors::error(
                ErrorKind::PtydUnavailable,
                "Invalid input registry",
            ));
        };
        for operation_key in receipts {
            let pending = self
                .store
                .input_intent_operation_pending(&operation_key)
                .map_err(errors::storage)?;
            if !pending
                && !crate::agents::delivery::receipt_pending(&self.store, &operation_key)
                    .map_err(errors::storage)?
            {
                self.pty_request(Request::ConfirmInput { operation_key })?;
            }
        }
        let Response::Instances(instances) = self.pty_request(Request::List)? else {
            return Err(errors::error(
                ErrorKind::PtydUnavailable,
                "Invalid registry response",
            ));
        };
        let ids: Vec<_> = self.panes.keys().cloned().collect();
        for id in ids {
            let saved = self.panes[&id].stored.clone();
            let existing = instances
                .iter()
                .find(|i| i.spawn.instance_key == saved.instance_key);
            if saved.closed {
                let child_keys: Vec<_> = self
                    .agents
                    .get(&id)
                    .map(|a| {
                        a.state
                            .terminals
                            .values()
                            .map(|t| t.spawn.instance_key.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                for instance in instances
                    .iter()
                    .filter(|i| !i.closed && child_keys.contains(&i.spawn.instance_key))
                {
                    self.pty_request(Request::Close {
                        instance_id: instance.instance_id.clone(),
                    })?;
                }
                if let Some(instance) = existing.filter(|i| !i.closed) {
                    self.pty_request(Request::Close {
                        instance_id: instance.instance_id.clone(),
                    })?;
                }
            } else if let Some(instance) = existing.filter(|i| !i.closed) {
                self.adopt(&id, instance)?;
            } else {
                if self.agents.contains_key(&id) {
                    if self.agents[&id].state.phase == crate::agents::state::Phase::Faulted {
                        self.close_agent_processes(&id)?;
                        continue;
                    }
                    self.recover_missing_agent(&id)?;
                    continue;
                }
                let auth_launch = self.store.has_pane_launch(&id).map_err(errors::storage)?;
                if auth_launch && saved.instance_id.is_some() {
                    let mut stopped = saved;
                    stopped.pane.pid = None;
                    stopped.pane.state = PaneState::Dead;
                    stopped.pane.phrase = "Sign-in process was lost".into();
                    self.save_runtime(&id, stopped)?;
                    continue;
                }
                if saved.instance_id.is_some() {
                    // A new supervisor cannot adopt an old process identifier.
                    // Reserve the replacement identity durably before spawning.
                    let mut next = saved;
                    next.pane.generation =
                        next.pane.generation.checked_add(1).ok_or_else(|| {
                            errors::error(ErrorKind::InternalError, "Pane generation exhausted")
                        })?;
                    next.pane.stream_id = Some(Uuid::new_v4().to_string());
                    next.pane.pid = None;
                    next.pane.state = PaneState::Recovering;
                    next.pane.phrase = "Restoring shell after process loss".into();
                    next.instance_key = Uuid::new_v4().to_string();
                    next.instance_id = None;
                    next.supervisor_id = None;
                    next.sequence_base = self.panes[&id].screen.next_offset();
                    self.save_runtime(&id, next)?;
                }
                self.spawn(&id)?;
            }
        }
        Ok(())
    }

    pub fn spawn(&mut self, id: &str) -> Result<()> {
        if self.agents.contains_key(id) {
            return self.spawn_agent(id);
        }
        let pane = &self.panes[id].stored;
        let launch: Option<Spawn> = self.store.pane_launch(id).map_err(errors::storage)?;
        let config = Spawn {
            instance_key: pane.instance_key.clone(),
            generation: pane.pane.generation,
            kind: ProcessKind::Pty,
            cwd: pane.pane.cwd.clone(),
            argv: launch
                .as_ref()
                .map(|l| l.argv.clone())
                .unwrap_or_else(|| vec!["/bin/zsh".into(), "-l".into()]),
            env: launch.map(|l| l.env).unwrap_or_default(),
            cols: pane.pane.cols as u16,
            rows: pane.pane.rows as u16,
        };
        let Response::Instance(instance) = self.pty_request(Request::Spawn(config))? else {
            return Err(errors::error(
                ErrorKind::PtydUnavailable,
                "Invalid spawn response",
            ));
        };
        self.adopt(id, &instance)
    }

    fn adopt(&mut self, id: &str, instance: &Instance) -> Result<()> {
        if self.agents.contains_key(id) {
            return self.adopt_agent(id, instance);
        }
        let mut saved = self.panes[id].stored.clone();
        saved.instance_id = Some(instance.instance_id.clone());
        saved.supervisor_id = Some(instance.supervisor_id.clone());
        saved.pane.pid = Some(instance.pid);
        saved.pane.state = if instance.exit_code.is_some() {
            PaneState::Dead
        } else {
            PaneState::Unknown
        };
        saved.pane.source = StateSource::None;
        saved.pane.confidence = 0.0;
        saved.pane.phrase = if instance.exit_code.is_some() {
            "Shell exited"
        } else {
            "Shell active"
        }
        .into();
        self.pty_request(Request::Resize {
            instance_id: instance.instance_id.clone(),
            cols: saved.pane.cols as u16,
            rows: saved.pane.rows as u16,
        })?;
        self.save_runtime(id, saved)
    }

    pub fn save_runtime(&mut self, id: &str, saved: StoredPane) -> Result<()> {
        let event = state_event(&saved);
        let change = self
            .store
            .mutate("engine", &Uuid::new_v4().to_string(), "runtime", |tx| {
                write_pane(tx, &saved)?;
                Ok((
                    json!({}),
                    vec![
                        ("pane.state_changed".into(), event),
                        ("pane.updated".into(), json!({"pane": saved.pane})),
                    ],
                ))
            })
            .map_err(errors::storage)?;
        self.panes.get_mut(id).expect("known pane").stored = saved;
        self.publish(&change.events);
        Ok(())
    }

    pub fn poll_output(&mut self) -> Result<()> {
        if self.ptyd.is_none() {
            self.connect_supervisor()?;
        }
        let ids: Vec<_> = self
            .panes
            .iter()
            .filter(|(_, p)| !p.stored.closed && p.stored.instance_id.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if self.agents.contains_key(&id) {
                if let Err(error) = self.poll_agent(&id) {
                    if matches!(
                        error.data.kind,
                        ErrorKind::InvalidParams | ErrorKind::HarnessUnavailable
                    ) {
                        self.fail_agent(&id, &error.message)?;
                    } else {
                        return Err(error);
                    }
                }
                continue;
            }
            let saved = self.panes[&id].stored.clone();
            let instance_id = saved.instance_id.clone().expect("adopted");
            let next = self.panes[&id].screen.next_offset();
            let seq = next.saturating_sub(saved.sequence_base);
            let Response::Output(output) = self.pty_request(Request::Drain {
                instance_id: instance_id.clone(),
                from_seq: seq,
            })?
            else {
                return Err(errors::error(
                    ErrorKind::PtydUnavailable,
                    "Invalid output response",
                ));
            };
            let running = self.panes.get_mut(&id).expect("known pane");
            let from = saved
                .sequence_base
                .checked_add(output.from_seq)
                .ok_or_else(|| {
                    errors::error(ErrorKind::InternalError, "Output offset exhausted")
                })?;
            if from > running.screen.next_offset() {
                running
                    .screen
                    .append(Record::Gap {
                        from: running.screen.next_offset(),
                        to: from,
                    })
                    .map_err(errors::io)?;
            }
            let bytes = STANDARD.decode(output.bytes).map_err(|_| {
                errors::error(ErrorKind::PtydUnavailable, "Invalid output encoding")
            })?;
            if !bytes.is_empty() {
                running
                    .screen
                    .append(Record::Data {
                        offset: from,
                        bytes,
                    })
                    .map_err(errors::io)?;
            }
            // Segment sync completed before returning ownership to the supervisor.
            self.pty_request(Request::Ack {
                instance_id,
                seq: output.next_seq,
            })?;
            if output.exited && saved.pane.state != PaneState::Dead {
                let mut dead = saved;
                dead.pane.state = PaneState::Dead;
                dead.pane.phrase = "Shell exited".into();
                self.save_runtime(&id, dead)?;
            }
        }
        Ok(())
    }
}

pub fn write_pane(tx: &Change<'_>, saved: &StoredPane) -> soyeht_store::database::Result<()> {
    tx.update_pane(saved)
}
pub fn state_event(saved: &StoredPane) -> Value {
    json!({"paneId":saved.pane.pane_id,"state":saved.pane.state,"source":saved.pane.source,"confidence":saved.pane.confidence,"phrase":saved.pane.phrase,"generation":saved.pane.generation})
}
