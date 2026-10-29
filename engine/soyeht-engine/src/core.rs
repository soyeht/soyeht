use crate::{
    errors::{self, Result},
    hooks::{Hooks, NoHooks},
    model::{self, Connected, RunningPane, StoredPane, Waiter},
    offload::SharedPort,
    ports::{Clock, StorePort, SystemClock},
    screen::Screen,
    supervisor_handle::Supervisor,
    tasks::{revision::GitRevision, worktree::CommandWorktree},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_platform::log::{self, Line};
use soyeht_protocol::{
    PaneState, PaneStateChangedDraft, StateSource, event,
    profile::{DEV, PTYD_SOCKET, Profile},
};
use soyeht_ptyd::{
    client::Client,
    wire::{Instance, ProcessKind, Request, Response, Spawn},
};
use soyeht_store::database::{Change, Event, Mutation, Store};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

pub struct Core {
    pub store: Store,
    pub directory: PathBuf,
    pub generation: u64,
    pub(crate) catalog_epoch: u64,
    pub ptyd: Option<Supervisor>,
    /// Requests to the supervisor nobody waits for, in the order they were made (see
    /// `Core::send_later`).
    pub(crate) supervisor_later: Vec<Request>,
    /// The last send to each process that is ordered against the ones after it (see
    /// `Core::turn`).
    pub(crate) send_turns: BTreeMap<String, Arc<crate::offload::Done>>,
    pub(crate) supervisor_retry: crate::supervisor_link::Retry,
    pub clock: Arc<dyn Clock>,
    /// What a test makes the engine do; [`NoHooks`] in a product (ADR 0091).
    pub hooks: Arc<dyn Hooks>,
    pub revision: SharedPort<dyn crate::tasks::revision::RevisionPort>,
    pub worktrees: SharedPort<dyn crate::tasks::worktree::WorktreePort>,
    pub(crate) background: crate::offload::Background,
    pub usage: crate::usage::Service,
    pub web: crate::web::Service,
    pub vcs: soyeht_vcs::Service,
    pub editor: crate::editor::Service,
    pub(crate) project_watches: crate::editor::ProjectWatches,
    pub(crate) skill_watches: crate::skills::Watches,
    pub languages: crate::languages::Service,
    pub(crate) task_revision_due: BTreeMap<(String, String), i64>,
    pub panes: BTreeMap<String, RunningPane>,
    pub(crate) terminal_captures: BTreeMap<String, crate::terminal_capture::TerminalCapture>,
    pub(crate) agent_captures: BTreeMap<String, crate::agent_capture::AgentCapture>,
    pub(crate) engine_build: &'static crate::engine_build::EngineBuildCell,
    /// Panes whose capture is on and waits for the engine build to restore (see `experiment_build`).
    pub(crate) capture_restore: BTreeSet<String>,
    /// When the panes in `capture_restore` stop waiting for the build.
    pub(crate) capture_restore_deadline: Option<std::time::Instant>,
    pub(crate) terminal_harnesses:
        BTreeMap<String, crate::terminal_harness::TerminalHarnessSession>,
    pub agents: BTreeMap<String, crate::agents::Running>,
    pub(crate) harness_switches: BTreeMap<String, crate::agents::switching::HarnessSwitchRuntime>,
    pub native_waiters: BTreeMap<(String, String), Vec<Waiter>>,
    pub language_waiters: BTreeMap<String, Vec<Waiter>>,
    pub web_waiters: BTreeMap<String, crate::web::WebWaiter>,
    pub clients: BTreeMap<String, Connected>,
    pub pair_address: String,
    /// Whether paired devices can reach this engine (tailnet listeners), ADR 0078.
    pub reachability: soyeht_protocol::EngineReachability,
    pub push: crate::push::Notifier,
}

/// Removes export temporaries a crashed engine left behind and logs the outcome.
fn log_stale_export_cleanup(directory: &Path) {
    match crate::experiment_export::cleanup_stale_export_temporaries_on_start(directory) {
        Ok((cleaned_files, cleaned_bytes)) if cleaned_files > 0 => {
            Line::new("engine.experiment_export_temporary_cleanup")
                .field("files", cleaned_files)
                .field("bytes", cleaned_bytes)
                .emit();
        }
        Err(error) => {
            log::failure("engine.experiment_export_temporary_cleanup_failed", &error);
        }
        _ => (),
    }
}

/// The address a paired device reaches the engine at before the listeners say otherwise.
fn loopback_address(profile: &Profile) -> String {
    format!("127.0.0.1:{}", profile.tcp_port)
}

impl Core {
    /// Makes the address this engine shows for pairing follow the port of `profile`.
    pub fn use_port_of(&mut self, profile: &Profile) {
        self.pair_address = loopback_address(profile);
    }

    pub fn open(directory: &Path) -> Result<Self> {
        Self::open_with(directory, Arc::new(NoHooks))
    }

    pub fn open_with(directory: &Path, hooks: Arc<dyn Hooks>) -> Result<Self> {
        // The user's interactive launch path, remembered from the last start: asking the login
        // shell takes seconds on a busy machine and the engine must not wait for it to listen.
        crate::agents::environment::warm_launch_path(directory);
        // The account's shell, which a terminal may need when `$SHELL` is unset, is read here and
        // not by the first terminal under the lock.
        soyeht_ptyd::shell::warm_default_login_shell();
        log_stale_export_cleanup(directory);
        let mut store = Store::open(directory).map_err(errors::storage)?;
        let workspaces = store.workspaces().map_err(errors::storage)?;
        for line in crate::workspace_layout::fallback_diagnostics(
            workspaces
                .iter()
                .map(|workspace| (workspace.workspace_id.as_str(), &workspace.layout)),
        ) {
            line.emit();
        }
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
            catalog_epoch: 0,
            ptyd: None,
            supervisor_later: Vec::new(),
            send_turns: BTreeMap::new(),
            supervisor_retry: Default::default(),
            clock: Arc::new(SystemClock),
            hooks: Arc::clone(&hooks),
            revision: SharedPort::new(Box::new(GitRevision)),
            worktrees: SharedPort::new(Box::new(CommandWorktree::default())),
            background: Default::default(),
            usage: crate::usage::service_over(usage_readings, hooks.usage_host()),
            web: crate::web::Service::new(Box::new(crate::web::SystemPortScan::default())),
            vcs: soyeht_vcs::Service::new(Box::new(soyeht_vcs::SystemGit)),
            editor: crate::editor::Service::new(
                hooks
                    .editor_disk()
                    .unwrap_or_else(|| Box::new(crate::editor::SystemFiles)),
            ),
            project_watches: Default::default(),
            skill_watches: Default::default(),
            languages: crate::languages::Service::restore(language_servers)?,
            task_revision_due: BTreeMap::new(),
            panes: BTreeMap::new(),
            terminal_captures: BTreeMap::new(),
            agent_captures: BTreeMap::new(),
            engine_build: crate::engine_build::cell(),
            capture_restore: BTreeSet::new(),
            capture_restore_deadline: None,
            terminal_harnesses: BTreeMap::new(),
            agents: BTreeMap::new(),
            harness_switches: BTreeMap::new(),
            native_waiters: BTreeMap::new(),
            language_waiters: BTreeMap::new(),
            web_waiters: BTreeMap::new(),
            clients: BTreeMap::new(),
            pair_address: loopback_address(&DEV),
            reachability: crate::network::loopback_only(),
            push: crate::push::Notifier::disabled(),
        };
        for stored in model::panes(&core.store)? {
            if stored.pane.mode == soyeht_protocol::PaneMode::Agent {
                let state = core.restore_agent(&stored)?;
                core.agents.insert(
                    stored.pane.pane_id.clone(),
                    crate::agents::Running::new(state),
                );
            }
            if stored.pane.mode == soyeht_protocol::PaneMode::Terminal
                && let Some(value) = core
                    .store
                    .pane_launch::<Value>(&stored.pane.pane_id)
                    .map_err(errors::storage)?
                && let Some(session) =
                    crate::terminal_harness::TerminalHarnessSession::decode(value)?
            {
                if session.spawn.instance_key != stored.instance_key
                    || session.spawn.generation != stored.pane.generation
                {
                    return Err(errors::internal(
                        "Stored terminal harness session does not match its pane generation",
                    ));
                }
                core.terminal_harnesses
                    .insert(stored.pane.pane_id.clone(), session);
            }
            let screen = Screen::open(
                &directory.join("sessions").join(&stored.pane.pane_id),
                stored.pane.record,
            )?;
            core.panes
                .insert(stored.pane.pane_id.clone(), RunningPane { stored, screen });
        }
        core.restore_experiment_captures()?;
        if let Err(error) = core.start_skills() {
            // Skills are optional: the engine serves everything else without them.
            log::failure("engine.skills_unavailable", &error);
        }
        let editor_restore_ms = core.now_ms();
        for file in editor_files {
            core.editor.restore(file, editor_restore_ms);
        }
        // The supervisor can start after the engine under the service manager.
        // No process is declared dead until its registry has been reconciled.
        // ignored: the engine is not shared yet; the tick connects again and logs a failure with
        // its streak (`ensure_supervisor`).
        let _ = core.connect_supervisor();
        core.system_event(event::ENGINE_RESTARTED, json!({"generation":generation}))?;
        Ok(core)
    }

    fn restore_agent(&mut self, stored: &StoredPane) -> Result<crate::agents::state::State> {
        let pane = &stored.pane.pane_id;
        let mut state: crate::agents::state::State = self
            .store
            .agent_runtime(pane)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::internal("Agent pane has no checkpoint"))?;
        // Older checkpoints encoded sign-in solely in the phase.
        let repaired_auth =
            state.phase == crate::agents::state::Phase::NeedsLogin && !state.auth_required;
        state.auth_required |= repaired_auth;
        if state.ensure_binding()? || repaired_auth {
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
        // Checked for every event, whether or not a client is subscribed to hear it.
        for event in events {
            model::check_event(&event.method, &event.params)?;
        }
        self.notify_push(events);
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

    /// Sends `request` to the supervisor and waits for the answer, with the Core lock held.
    /// Only for what has not moved off the lock yet; `tests/blocking_io_rule.rs` counts the
    /// callers (ADR 0117).
    pub fn pty_request(&mut self, request: Request) -> Result<Response> {
        let supervisor = self.supervisor()?;
        let result = supervisor.call(request);
        self.settle_supervisor(&supervisor, result)
    }

    /// Drops the link to the supervisor and records `ptyd.lost`. A lost event that cannot be
    /// recorded must at least reach the log.
    pub(crate) fn lose_supervisor_link(&mut self) {
        self.ptyd = None;
        self.supervisor_later.clear();
        if let Err(error) = self.system_event(
            event::PTYD_LOST,
            json!({"reason":"Supervisor connection closed"}),
        ) {
            log::failure("engine.ptyd_lost_unrecorded", &error);
        }
    }

    pub fn connect_supervisor(&mut self) -> Result<()> {
        let client = Client::connect(&self.directory.join(PTYD_SOCKET), self.generation)
            .map_err(|_| errors::ptyd_unavailable("Supervisor unavailable"))?;
        self.adopt_supervisor(client)
    }

    /// Makes `client` the link, reconciles the registry with the panes and records the
    /// reconnection; the link is dropped again when the reconciliation fails.
    pub(crate) fn adopt_supervisor(&mut self, client: Client) -> Result<()> {
        self.link_supervisor(Box::new(client));
        let result = self.reconcile_registry();
        if result.is_err() {
            self.ptyd = None;
        } else {
            let generation = errors::invariant(
                self.ptyd.as_ref(),
                "Supervisor disappeared after registry reconciliation",
            )?
            .supervisor_generation();
            self.system_event(event::PTYD_RECONNECTED, json!({"generation":generation}))?;
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
            if let Err(error) = self.reconcile_pane(&id, &instances, &reserved_panes) {
                self.stop_unrestartable_pane(&id, error)?;
            }
        }
        self.close_orphaned_agent_processes(&instances)?;
        self.reconcile_language_servers(&instances)
    }

    fn reconcile_pane(
        &mut self,
        id: &str,
        instances: &[Instance],
        reserved_panes: &std::collections::BTreeSet<String>,
    ) -> Result<()> {
        let saved = self.panes[id].stored.clone();
        if matches!(
            saved.pane.mode,
            soyeht_protocol::PaneMode::Editor
                | soyeht_protocol::PaneMode::Web
                | soyeht_protocol::PaneMode::Writing
        ) || (saved.instance_id.is_none() && reserved_panes.contains(id))
        {
            return Ok(());
        }
        let existing = instances
            .iter()
            .find(|instance| instance.spawn.instance_key == saved.instance_key);
        if saved.closed {
            let child_keys: Vec<_> = self
                .agents
                .get(id)
                .map(|agent| {
                    agent
                        .state
                        .terminals
                        .values()
                        .map(|terminal| terminal.spawn.instance_key.clone())
                        .collect()
                })
                .unwrap_or_default();
            for instance in instances.iter().filter(|instance| {
                !instance.closed && child_keys.contains(&instance.spawn.instance_key)
            }) {
                self.pty_request(Request::Close {
                    instance_id: instance.instance_id.clone(),
                })?;
            }
            if let Some(instance) = existing.filter(|instance| !instance.closed) {
                self.pty_request(Request::Close {
                    instance_id: instance.instance_id.clone(),
                })?;
            }
            return Ok(());
        }
        if let Some(instance) = existing.filter(|instance| !instance.closed) {
            return self.adopt(id, instance);
        }
        if self.agents.contains_key(id) {
            if self.agents[id].state.phase == crate::agents::state::Phase::Faulted {
                return self.close_agent_processes(id);
            }
            return self.recover_missing_agent(id);
        }
        if self.terminal_harnesses.contains_key(id) && saved.instance_id.is_some() {
            let mut stopped = saved;
            stopped.pane.pid = None;
            stopped.pane.state = PaneState::Dead;
            stopped.pane.phrase = "Terminal harness exited".into();
            return self.save_runtime(id, stopped);
        }
        let auth_launch = StorePort::has_pane_launch(&self.store, id).map_err(errors::storage)?;
        if auth_launch && saved.pane.state == PaneState::Dead {
            // Already reported lost; a sign-in terminal is never started again by recovery.
            return Ok(());
        }
        if auth_launch && saved.instance_id.is_some() {
            // The pane keeps its output for review but no longer names a process the
            // supervisor does not have, which would fail every output poll after it.
            let mut stopped = saved;
            stopped.pane.pid = None;
            stopped.instance_id = None;
            stopped.supervisor_id = None;
            stopped.pane.state = PaneState::Dead;
            stopped.pane.phrase = crate::agents::authentication::SIGN_IN_PROCESS_LOST.into();
            return self.save_runtime(id, stopped);
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
            next.sequence_base = self.panes[id].screen.next_offset();
            self.save_runtime(id, next)?;
        }
        self.spawn(id)
    }

    pub fn spawn(&mut self, id: &str) -> Result<()> {
        if self.agents.contains_key(id) {
            return self.spawn_agent(id);
        }
        let pane = &self.panes[id].stored;
        let managed = self
            .terminal_harnesses
            .get(id)
            .map(|session| session.spawn.clone());
        let launch: Option<Spawn> = if managed.is_some() {
            managed
        } else {
            StorePort::pane_launch(&self.store, id).map_err(errors::storage)?
        };
        let config = Spawn {
            instance_key: pane.instance_key.clone(),
            generation: pane.pane.generation,
            kind: ProcessKind::Pty,
            cwd: pane.pane.cwd.clone(),
            argv: launch
                .as_ref()
                .map(|l| l.argv.clone())
                .unwrap_or_else(soyeht_ptyd::shell::default_login_shell),
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
        saved.pane.exit_code = instance.exit_code.map(i64::from);
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
        let state_changed = state_event(&saved)?;
        let updated = model::pane_updated(&saved.pane)?;
        let change = StorePort::mutate(
            &mut self.store,
            "engine",
            &Uuid::new_v4().to_string(),
            "runtime",
            |tx| {
                write_pane(tx, &saved)?;
                Ok((json!({}), vec![state_changed, updated]))
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

    /// One engine tick, under the Core lock. Blocking work never runs here
    /// (ADR 0079). A failing subsystem is reported under its own name and
    /// never stops the terminal drain or the subsystems after it.
    pub fn poll_output(&mut self) -> Result<()> {
        self.apply_background();
        self.isolated("editor", Self::advance_editor);
        self.isolated("project_tree", Self::advance_project_tree);
        self.isolated("skills", Self::advance_skills);
        self.isolated("local_servers", Self::advance_local_servers);
        self.isolated("web_commands", Self::advance_web_commands);
        if self.ensure_supervisor() == crate::supervisor_link::Link::Down {
            return Ok(());
        }
        self.flush_later();
        self.isolated("spawn_retries", Self::advance_spawn_retries);
        self.isolated("close_retries", Self::advance_close_retries);
        self.isolated("harness_switches", Self::advance_harness_switches);
        self.isolated("retired_agents", Self::cleanup_retired_agent_processes);
        self.isolated("experiment_captures", Self::reconcile_experiment_captures);
        let drained = self.drain_panes();
        self.advance_language_servers();
        self.isolated("task_reviews", Self::advance_task_reviews);
        self.isolated("task_feedback", Self::advance_task_feedback);
        self.isolated("task_blocking", Self::advance_task_blocking);
        self.isolated("task_dispatches", Self::advance_task_dispatches);
        self.isolated("task_loops", Self::advance_task_loops);
        drained
    }

    fn isolated(&mut self, subsystem: &'static str, step: fn(&mut Self) -> Result<()>) {
        let result = step(self);
        self.background.report(subsystem, &result);
    }

    /// Moves new terminal and agent output from the supervisor into each
    /// pane's screen. This is the terminal path.
    pub(crate) fn drain_panes(&mut self) -> Result<()> {
        let ids: Vec<_> = self
            .panes
            .iter()
            .filter(|(_, p)| !p.stored.closed && p.stored.instance_id.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            // Its output waits in the supervisor until the capture it belongs to is restored.
            if self.capture_restore_pending(&id) {
                continue;
            }
            if self.agents.contains_key(&id) {
                if let Err(error) = self.poll_agent(&id) {
                    if errors::missing_supervisor_instance(&error) {
                        // A respawn that fails leaves the pane without a process: it is
                        // retried or stopped, not returned.
                        let recovered = self.recover_missing_agent(&id);
                        self.settle_spawn(&id, recovered);
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
            self.drain_terminal(&id)?;
        }
        Ok(())
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

/// Writes a pane and, once it is closed, releases any client focus on it.
pub fn persist_pane(tx: &Change<'_>, saved: &StoredPane) -> soyeht_store::database::Result<()> {
    write_pane(tx, saved)?;
    if saved.closed {
        tx.close_pane(&saved.pane.pane_id)?;
    }
    Ok(())
}
pub fn state_event(saved: &StoredPane) -> Result<(String, Value)> {
    model::pending(&PaneStateChangedDraft {
        pane_id: saved.pane.pane_id.clone(),
        state: saved.pane.state.clone(),
        source: saved.pane.source.clone(),
        confidence: saved.pane.confidence,
        phrase: saved.pane.phrase.clone(),
    })
}
