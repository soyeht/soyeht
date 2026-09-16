use crate::{
    errors::{self, Error, Result},
    model::{self, Connected, RunningPane, StoredPane},
    ports::{Clock, StorePort, SupervisorPort, SystemClock, call_supervisor},
    screen::Screen,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::{
    PaneState, StateSource,
    profile::{DEV_PTYD_SOCKET, DEV_TCP_PORT},
};
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
    sync::Arc,
};
use uuid::Uuid;

pub struct Core {
    pub store: Store,
    pub directory: PathBuf,
    pub generation: u64,
    pub ptyd: Option<Box<dyn SupervisorPort>>,
    pub clock: Arc<dyn Clock>,
    pub revision: Box<dyn crate::tasks::revision::RevisionPort>,
    pub worktrees: Box<dyn crate::tasks::worktree::WorktreePort>,
    pub usage: crate::usage::Service,
    pub web: crate::web::Service,
    pub vcs: soyeht_vcs::Service,
    pub editor: crate::editor::Service,
    pub languages: crate::languages::Service,
    pub(crate) task_revision_due: BTreeMap<(String, String), i64>,
    pub panes: BTreeMap<String, RunningPane>,
    pub(crate) terminal_captures: BTreeMap<String, crate::terminal_capture::TerminalCapture>,
    pub(crate) terminal_capture_build: Option<soyeht_pane_state::EngineBuild>,
    pub agents: BTreeMap<String, crate::agents::Running>,
    pub(crate) harness_switches: BTreeMap<String, crate::agents::switching::HarnessSwitchRuntime>,
    pub native_waiters: BTreeMap<(String, String), Vec<(String, Value)>>,
    pub language_waiters: BTreeMap<String, Vec<(String, Value)>>,
    pub web_waiters: BTreeMap<String, crate::web::WebWaiter>,
    pub clients: BTreeMap<String, Connected>,
    pub pair_address: String,
}

impl Core {
    pub fn open(directory: &Path) -> Result<Self> {
        // Resolve the user's interactive launch path before requests arrive so
        // catalogue reads and process launches share one environment snapshot.
        let _ = crate::agents::environment::launch_path();
        match crate::experiment_export::cleanup_stale_export_temporaries_on_start(directory) {
            Ok((cleaned_files, cleaned_bytes)) if cleaned_files > 0 => {
                eprintln!(
                    "{}",
                    json!({
                        "event":"engine.experiment_export_temporary_cleanup",
                        "files":cleaned_files,
                        "bytes":cleaned_bytes.to_string()
                    })
                );
            }
            Err(error) => {
                eprintln!(
                    "{}",
                    json!({
                        "event":"engine.experiment_export_temporary_cleanup_failed",
                        "reason":error.to_string()
                    })
                );
            }
            _ => (),
        }
        let mut store = Store::open(directory).map_err(errors::storage)?;
        let generation = store.next_engine_generation().map_err(errors::storage)?;
        if generation > u32::MAX as u64 {
            return Err(errors::internal("Engine generation exhausted"));
        }
        let usage_readings = store.usage_readings().map_err(errors::storage)?;
        let editor_files = store.editor_files().map_err(errors::storage)?;
        let language_servers = store
            .language_server_checkpoints()
            .map_err(errors::storage)?;
        let mut core = Self {
            store,
            directory: directory.into(),
            generation,
            ptyd: None,
            clock: Arc::new(SystemClock),
            revision: Box::new(crate::tasks::revision::GitRevision),
            worktrees: Box::new(crate::tasks::worktree::CommandWorktree::default()),
            usage: crate::usage::Service::restore(
                usage_readings,
                Box::new(crate::usage::SystemSources::default()),
            ),
            web: crate::web::Service::new(Box::new(crate::web::SystemPortScan::default())),
            vcs: soyeht_vcs::Service::new(Box::new(soyeht_vcs::SystemGit)),
            editor: crate::editor::Service::new(Box::new(crate::editor::SystemFiles)),
            languages: crate::languages::Service::restore(language_servers)?,
            task_revision_due: BTreeMap::new(),
            panes: BTreeMap::new(),
            terminal_captures: BTreeMap::new(),
            terminal_capture_build: None,
            agents: BTreeMap::new(),
            harness_switches: BTreeMap::new(),
            native_waiters: BTreeMap::new(),
            language_waiters: BTreeMap::new(),
            web_waiters: BTreeMap::new(),
            clients: BTreeMap::new(),
            pair_address: format!("127.0.0.1:{DEV_TCP_PORT}"),
        };
        for stored in model::panes(&core.store)? {
            if stored.pane.mode == soyeht_protocol::PaneMode::Agent {
                let state = core.restore_agent(&stored)?;
                core.agents.insert(
                    stored.pane.pane_id.clone(),
                    crate::agents::Running::new(state),
                );
            }
            let screen = Screen::open(
                &directory.join("sessions").join(&stored.pane.pane_id),
                stored.pane.record,
            )?;
            core.panes
                .insert(stored.pane.pane_id.clone(), RunningPane { stored, screen });
        }
        core.restore_terminal_captures()?;
        let editor_restore_ms = core.now_ms();
        for file in editor_files {
            core.editor.restore(file, editor_restore_ms);
        }
        // The supervisor can start after the engine under the service manager.
        // No process is declared dead until its registry has been reconciled.
        let _ = core.connect_supervisor();
        core.system_event("engine.restarted", json!({"generation":generation}))?;
        Ok(core)
    }

    fn restore_agent(&mut self, stored: &StoredPane) -> Result<crate::agents::state::State> {
        let pane = &stored.pane.pane_id;
        let mut state: crate::agents::state::State = self
            .store
            .agent_runtime(pane)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::internal("Agent pane has no checkpoint"))?;
        if state.ensure_binding()? {
            StorePort::mutate(
                &mut self.store,
                "engine-agent",
                &Uuid::new_v4().to_string(),
                "agent.binding.repair",
                |change| {
                    soyeht_store::agents::save_runtime(
                        change,
                        pane,
                        stored.pane.generation,
                        &state,
                    )?;
                    Ok((json!({}), Vec::new()))
                },
            )
            .map_err(errors::storage)?;
        }
        state.restore_queue(self.store.agent_queue(pane).map_err(errors::storage)?);
        Ok(state)
    }

    pub(crate) fn system_event(&mut self, method: &str, params: Value) -> Result<()> {
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine",
            &Uuid::new_v4().to_string(),
            method,
            |_| Ok((json!({}), vec![(method.into(), params)])),
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)
    }

    pub(crate) fn transient_event(&self, method: &str, params: Value) -> Result<()> {
        let transient = soyeht_protocol::EVENTS
            .iter()
            .any(|event| event.name.as_str() == method && !event.durable);
        if !transient || !params.is_object() || params.get("eventId").is_some() {
            return Err(errors::internal("Invalid transient event"));
        }
        let frame = model::notification(method, params)?;
        for client in self
            .clients
            .values()
            .filter(|client| client.events && client.actor.is_some())
        {
            client.outbox.send(frame.clone());
        }
        Ok(())
    }

    pub fn publish(&self, events: &[Event]) -> Result<()> {
        for client in self
            .clients
            .values()
            .filter(|c| c.events && c.actor.is_some())
        {
            for event in events {
                client
                    .outbox
                    .send(model::notification(&event.method, event.params.clone())?);
            }
        }
        Ok(())
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
            .ok_or_else(|| errors::unauthorized("Connection not found"))?;
        let actor = serde_json::to_value(
            client
                .actor
                .as_ref()
                .ok_or_else(|| errors::unauthorized("Handshake required"))?,
        )
        .map_err(|_| errors::internal("Connection actor serialization failed"))?;
        let key = params["idempotencyKey"]
            .as_str()
            .ok_or_else(|| errors::invalid("Missing idempotency key"))?;
        let fingerprint = STANDARD.encode(crate::auth::token_hash(
            &json!([method, params]).to_string(),
        ));
        let mutation = StorePort::mutate_as(
            &mut self.store,
            &client.scope,
            key,
            &fingerprint,
            Some(&actor),
            change,
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)?;
        Ok(mutation)
    }

    pub(crate) fn repeated_mutation(
        &self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Option<Value>> {
        let client = self
            .clients
            .get(connection)
            .ok_or_else(|| errors::unauthorized("Connection not found"))?;
        let key = params["idempotencyKey"]
            .as_str()
            .ok_or_else(|| errors::invalid("Missing idempotency key"))?;
        let fingerprint = STANDARD.encode(crate::auth::token_hash(
            &json!([method, params]).to_string(),
        ));
        let receipt = StorePort::idempotency_receipt(&self.store, &client.scope, key)
            .map_err(errors::storage)?;
        match receipt {
            Some(receipt) if receipt.fingerprint == fingerprint => Ok(Some(receipt.result)),
            Some(_) => Err(errors::idempotency_conflict(
                "Idempotency key belongs to another mutation",
            )),
            None => Ok(None),
        }
    }

    pub fn pty_request(&mut self, request: Request) -> Result<Response> {
        let result = call_supervisor(
            self.ptyd
                .as_mut()
                .ok_or_else(|| errors::ptyd_unavailable("Supervisor disconnected"))?
                .as_mut(),
            request,
        );
        match result {
            Err(error @ Error::Supervisor(soyeht_ptyd::Error::Remote { .. })) => Err(error),
            Ok(response) => Ok(response),
            Err(Error::Supervisor(_)) => {
                self.ptyd = None;
                let _ = self.system_event(
                    "ptyd.lost",
                    json!({"reason":"Supervisor connection closed"}),
                );
                Err(errors::ptyd_unavailable("Supervisor disconnected"))
            }
            Err(error) => Err(error),
        }
    }

    pub fn connect_supervisor(&mut self) -> Result<()> {
        let client = Client::connect(&self.directory.join(DEV_PTYD_SOCKET), self.generation)
            .map_err(|_| errors::ptyd_unavailable("Supervisor unavailable"))?;
        self.ptyd = Some(Box::new(client));
        let result = self.reconcile_registry();
        if result.is_err() {
            self.ptyd = None;
        } else {
            let generation = errors::invariant(
                self.ptyd.as_ref(),
                "Supervisor disappeared after registry reconciliation",
            )?
            .supervisor_generation();
            self.system_event("ptyd.reconnected", json!({"generation":generation}))?;
        }
        result
    }

    fn reconcile_registry(&mut self) -> Result<()> {
        let Response::InputReceipts(receipts) = self.pty_request(Request::PendingInputs)? else {
            return Err(errors::ptyd_unavailable("Invalid input registry"));
        };
        for operation_key in receipts {
            let pending = StorePort::input_intent_operation_pending(&self.store, &operation_key)
                .map_err(errors::storage)?;
            if !pending
                && !crate::agents::delivery::receipt_pending(&self.store, &operation_key)?
                && !self.languages.has_pending_write(&operation_key)
            {
                self.pty_request(Request::ConfirmInput { operation_key })?;
            }
        }
        let Response::Instances(instances) = self.pty_request(Request::List)? else {
            return Err(errors::ptyd_unavailable("Invalid registry response"));
        };
        self.recover_harness_switches(&instances)?;
        self.cleanup_retired_agent_processes()?;
        let reserved_panes: std::collections::BTreeSet<_> = self
            .store
            .dispatch_checkpoints()
            .map_err(errors::storage)?
            .into_iter()
            .filter(|(_, _, checkpoint)| {
                matches!(checkpoint.stage.as_str(), "reserved" | "worktree_created")
            })
            .filter_map(|(_, _, checkpoint)| checkpoint.pane_id)
            .collect();
        let ids: Vec<_> = self.panes.keys().cloned().collect();
        for id in ids {
            let saved = self.panes[&id].stored.clone();
            if matches!(
                saved.pane.mode,
                soyeht_protocol::PaneMode::Editor
                    | soyeht_protocol::PaneMode::Web
                    | soyeht_protocol::PaneMode::Writing
            ) {
                continue;
            }
            if saved.instance_id.is_none() && reserved_panes.contains(&id) {
                continue;
            }
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
                let auth_launch =
                    StorePort::has_pane_launch(&self.store, &id).map_err(errors::storage)?;
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
                    next.pane.generation = next
                        .pane
                        .generation
                        .checked_add(1)
                        .ok_or_else(|| errors::internal("Pane generation exhausted"))?;
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
        self.reconcile_language_servers(&instances)
    }

    pub fn spawn(&mut self, id: &str) -> Result<()> {
        if self.agents.contains_key(id) {
            return self.spawn_agent(id);
        }
        let pane = &self.panes[id].stored;
        let launch: Option<Spawn> =
            StorePort::pane_launch(&self.store, id).map_err(errors::storage)?;
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
            return Err(errors::ptyd_unavailable("Invalid spawn response"));
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
        let change = StorePort::mutate(
            &mut self.store,
            "engine",
            &Uuid::new_v4().to_string(),
            "runtime",
            |tx| {
                write_pane(tx, &saved)?;
                Ok((
                    json!({}),
                    vec![
                        ("pane.state_changed".into(), event),
                        ("pane.updated".into(), json!({"pane": saved.pane})),
                    ],
                ))
            },
        )
        .map_err(errors::storage)?;
        errors::invariant(
            self.panes.get_mut(id),
            "Pane disappeared before runtime publication",
        )?
        .stored = saved;
        self.publish(&change.events)
    }

    pub fn poll_output(&mut self) -> Result<()> {
        self.advance_editor()?;
        self.advance_local_servers()?;
        self.advance_web_commands()?;
        if self.ptyd.is_none() {
            self.connect_supervisor()?;
        }
        self.advance_harness_switches()?;
        self.cleanup_retired_agent_processes()?;
        self.reconcile_terminal_captures()?;
        let ids: Vec<_> = self
            .panes
            .iter()
            .filter(|(_, p)| !p.stored.closed && p.stored.instance_id.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if self.agents.contains_key(&id) {
                if let Err(error) = self.poll_agent(&id) {
                    if errors::missing_supervisor_instance(&error) {
                        self.recover_missing_agent(&id)?;
                    } else if errors::agent_poll_failure_is_global(&error) {
                        return Err(error);
                    } else {
                        // A pane-owned failure must not starve panes that sort
                        // after it. Failure publication and process retirement
                        // are durable; an error there still stops the cycle.
                        self.fail_agent(&id, &error.to_string())?;
                    }
                }
                continue;
            }
            let saved = self.panes[&id].stored.clone();
            let instance_id = errors::invariant(
                saved.instance_id.clone(),
                "Running pane has no adopted process",
            )?;
            let next = self.panes[&id].screen.next_offset();
            let seq = next.saturating_sub(saved.sequence_base);
            let Response::Output(output) = self.pty_request(Request::Drain {
                instance_id: instance_id.clone(),
                from_seq: seq,
            })?
            else {
                return Err(errors::ptyd_unavailable("Invalid output response"));
            };
            let running = errors::invariant(
                self.panes.get_mut(&id),
                "Pane disappeared while polling output",
            )?;
            let from = saved
                .sequence_base
                .checked_add(output.from_seq)
                .ok_or_else(|| errors::internal("Output offset exhausted"))?;
            if from > running.screen.next_offset() {
                running.screen.append(Record::Gap {
                    from: running.screen.next_offset(),
                    to: from,
                })?;
            }
            let bytes = STANDARD
                .decode(output.bytes)
                .map_err(|_| errors::ptyd_unavailable("Invalid output encoding"))?;
            let capture_bytes = if !bytes.is_empty() && self.terminal_captures.contains_key(&id) {
                let shared = Arc::new(bytes);
                running
                    .screen
                    .append_shared_data(from, Arc::clone(&shared))?;
                Some(shared)
            } else if !bytes.is_empty() {
                running.screen.append(Record::Data {
                    offset: from,
                    bytes,
                })?;
                None
            } else {
                None
            };
            // Segment sync completed before returning ownership to the supervisor.
            self.pty_request(Request::Ack {
                instance_id,
                seq: output.next_seq,
            })?;
            if let Some(bytes) = capture_bytes {
                self.observe_terminal(&id, bytes)?;
            }
            if output.exited && saved.pane.state != PaneState::Dead {
                let mut dead = saved;
                dead.pane.state = PaneState::Dead;
                dead.pane.phrase = "Shell exited".into();
                self.save_runtime(&id, dead)?;
            }
        }
        self.advance_language_servers();
        self.advance_task_reviews()?;
        self.advance_task_feedback()?;
        self.advance_task_blocking()?;
        self.advance_task_dispatches()?;
        self.advance_task_loops()
    }

    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    pub fn now_unix(&self) -> i64 {
        self.clock.now_unix()
    }

    pub fn timestamp(&self) -> String {
        self.clock.timestamp()
    }
}

pub fn write_pane(tx: &Change<'_>, saved: &StoredPane) -> soyeht_store::database::Result<()> {
    tx.update_pane(saved)
}
pub fn state_event(saved: &StoredPane) -> Value {
    json!({"paneId":saved.pane.pane_id,"state":saved.pane.state,"source":saved.pane.source,"confidence":saved.pane.confidence,"phrase":saved.pane.phrase,"generation":saved.pane.generation})
}
