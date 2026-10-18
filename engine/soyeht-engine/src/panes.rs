use crate::{
    core::{Core, persist_pane},
    errors::{self, Result},
    model::{self, RunningPane, StoredPane},
    ports::StorePort,
    screen::Screen,
    settings,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Pane, PaneExperimentCapture, PaneMode, PaneState, StateSource, event, method, setting,
};
use soyeht_ptyd::wire::Request;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, PartialEq, Eq)]
struct PaneDefaults {
    cols: u32,
    rows: u32,
    record: bool,
}

enum PreparedExperimentCapture {
    Terminal(crate::terminal_capture::TerminalCapture),
    Agent(crate::agent_capture::AgentCapture),
}

fn pane_defaults(store: &impl StorePort) -> Result<PaneDefaults> {
    let cols = settings::read(store, setting::TERMINAL_DEFAULT_COLS)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| errors::internal("Stored terminal column setting is invalid"))?;
    let rows = settings::read(store, setting::TERMINAL_DEFAULT_ROWS)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| errors::internal("Stored terminal row setting is invalid"))?;
    let record = settings::read(store, setting::RECORD_DEFAULT)?
        .as_bool()
        .ok_or_else(|| errors::internal("Stored recording setting is invalid"))?;
    Ok(PaneDefaults { cols, rows, record })
}

pub(crate) fn persist_new_pane(
    change: &soyeht_store::database::Change<'_>,
    saved: &StoredPane,
    agent: Option<&crate::agents::state::State>,
) -> soyeht_store::database::Result<()> {
    change.insert_pane(saved)?;
    if let Some(agent) = agent {
        soyeht_store::agents::save_runtime(
            change,
            &saved.pane.pane_id,
            saved.pane.generation,
            agent,
        )?;
    }
    Ok(())
}

pub(crate) fn pane_created(saved: &StoredPane) -> Result<(Value, Vec<(String, Value)>)> {
    Ok((
        json!({"paneId":saved.pane.pane_id}),
        vec![model::pane_created(&saved.pane)?],
    ))
}

fn new_agent(
    directory: &Path,
    store: &impl StorePort,
    params: &Value,
) -> Result<Option<crate::agents::state::State>> {
    if params["mode"] != "agent" {
        return Ok(None);
    }
    let harness = params["harness"]
        .as_str()
        .ok_or_else(|| errors::invalid("Agent panes require a harness"))?;
    let (launch, digest) = crate::agents::registry::prepare(directory, harness)?;
    let mode = match params.get("permissionMode") {
        Some(Value::String(mode)) => Some(mode.clone()),
        Some(_) => return Err(errors::invalid("Invalid permission mode")),
        None => settings::default_agent_mode(store, harness)?,
    };
    let mut agent = crate::agents::state::State::new(launch, digest, mode)?;
    for name in ["model", "effort"] {
        if let Some(value) = params[name].as_str() {
            agent
                .requested_options
                .insert(name.into(), soyeht_protocol::ConfigValue::V0(value.into()));
        }
    }
    Ok(Some(agent))
}

impl Core {
    fn prepare_process_pane(
        &self,
        params: &Value,
        defaults: &PaneDefaults,
        agent: Option<&crate::agents::state::State>,
    ) -> Result<(
        StoredPane,
        Option<(crate::terminal_harness::TerminalHarnessSession, String)>,
    )> {
        let agent_mode = params["mode"] == "agent";
        let workspace = model::workspace(&self.store, errors::param_str(params, "workspaceId")?)?;
        let requested_cwd = params["cwd"].as_str().unwrap_or(&workspace.path);
        if !Path::new(requested_cwd).is_absolute() || !Path::new(requested_cwd).is_dir() {
            return Err(errors::invalid("Pane directory must exist and be absolute"));
        }
        // Always the platform's canonical form: a client may send any spelling of the folder,
        // including the extended `\\?\C:\...` one that `cmd.exe` cannot start in.
        let canonical =
            soyeht_platform::fs::canonicalize(Path::new(requested_cwd)).map_err(errors::io)?;
        let cwd = canonical
            .to_str()
            .ok_or_else(|| errors::invalid("Working directory must be UTF-8"))?;
        let name = match (params["name"].as_str(), agent) {
            (Some(name), _) => name.to_owned(),
            (None, Some(state)) => self.default_agent_name(
                &workspace.workspace_id,
                crate::agents::naming::harness_label(&state.launch),
                None,
            ),
            (None, None) => "Shell".into(),
        };
        let instance_key = Uuid::new_v4().to_string();
        let mut pane = Pane {
            agent: agent.map(|state| state.public(self.now_ms())),
            url: None,
            title: None,
            viewport: None,
            incognito: None,
            writing_kind: None,
            writing_section: None,
            exit_code: None,
            web_session: None,
            pane_id: Uuid::new_v4().to_string(),
            workspace_id: workspace.workspace_id,
            cwd: cwd.into(),
            name,
            mode: if agent_mode {
                PaneMode::Agent
            } else {
                PaneMode::Terminal
            },
            state: PaneState::Starting,
            source: if agent_mode {
                StateSource::Protocol
            } else {
                StateSource::None
            },
            confidence: if agent_mode { 1.0 } else { 0.0 },
            phrase: if agent_mode {
                "Starting agent"
            } else {
                "Starting shell"
            }
            .into(),
            generation: 1,
            cols: params["cols"]
                .as_u64()
                .map_or(defaults.cols, |value| value as u32),
            rows: params["rows"]
                .as_u64()
                .map_or(defaults.rows, |value| value as u32),
            record: agent_mode || defaults.record,
            experiment_capture: Some(PaneExperimentCapture {
                on: false,
                failure: None,
            }),
            pid: None,
            stream_id: (!agent_mode).then(|| Uuid::new_v4().to_string()),
        };
        let terminal_harness = params["harness"]
            .as_str()
            .filter(|_| !agent_mode && params["mode"] == "terminal")
            .map(|harness| {
                crate::terminal_harness::TerminalHarnessSession::prepare(
                    &self.directory,
                    harness,
                    instance_key.clone(),
                    pane.generation,
                    pane.cwd.clone(),
                    u16::try_from(pane.cols)
                        .map_err(|_| errors::invalid("Pane columns exceed the terminal range"))?,
                    u16::try_from(pane.rows)
                        .map_err(|_| errors::invalid("Pane rows exceed the terminal range"))?,
                )
            })
            .transpose()?;
        if params["name"].as_str().is_none()
            && let Some((_, harness_name)) = &terminal_harness
        {
            pane.name = format!("{harness_name} Terminal");
        }
        Ok((
            StoredPane {
                pane,
                closed: false,
                instance_key,
                instance_id: None,
                supervisor_id: None,
                sequence_base: 0,
            },
            terminal_harness,
        ))
    }

    pub fn pane_create(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if params["mode"] == "editor" {
            return self.editor_pane_create(connection, params);
        }
        if params["mode"] == "web" {
            return self.web_pane_create(connection, params);
        }
        if params["mode"] == "writing" {
            return self.writing_pane_create(connection, params);
        }
        let defaults = pane_defaults(&self.store)?;
        let agent = new_agent(&self.directory, &self.store, params)?;
        let (saved, terminal_harness) =
            self.prepare_process_pane(params, &defaults, agent.as_ref())?;
        // Open storage before accepting a process creation. No child exists yet.
        let screen = Screen::open(
            &self.directory.join("sessions").join(&saved.pane.pane_id),
            saved.pane.record,
        )?;
        let created = pane_created(&saved)?;
        let mutation = self.mutate(connection, method::PANE_CREATE, params, |tx| {
            persist_new_pane(tx, &saved, agent.as_ref())?;
            if let Some((session, _)) = &terminal_harness {
                tx.insert_pane_launch(&saved.pane.pane_id, session)?;
            }
            Ok(created)
        })?;
        if !mutation.repeated {
            let id = saved.pane.pane_id.clone();
            self.panes.insert(
                id.clone(),
                RunningPane {
                    stored: saved,
                    screen,
                },
            );
            if let Some(agent) = agent {
                self.agents
                    .insert(id.clone(), crate::agents::Running::new(agent));
            }
            if let Some((session, _)) = terminal_harness {
                self.terminal_harnesses.insert(id.clone(), session);
            }
            // A disconnected supervisor leaves a durable creation intent for
            // reconciliation, never a second process on an RPC retry.
            if self.spawn(&id).is_err() {
                self.ptyd = None;
            }
        }
        Ok(mutation.result)
    }

    pub(crate) fn prepare_pane_close(
        &self,
        saved: &mut StoredPane,
        events: &mut Vec<(String, Value)>,
    ) -> Result<Option<soyeht_protocol::Workspace>> {
        saved.closed = true;
        let workspace =
            self.workspace_after_closing_pane(&saved.pane.workspace_id, &saved.pane.pane_id)?;
        events.push(model::pane_closed(
            &saved.pane.pane_id,
            &saved.pane.workspace_id,
        )?);
        if let Some(workspace) = &workspace {
            events.push((
                event::WORKSPACE_LAYOUT_UPDATED.into(),
                json!({"workspace":workspace}),
            ));
        }
        Ok(workspace)
    }

    pub fn pane_edit(&mut self, connection: &str, method: &str, params: &Value) -> Result<Value> {
        if method == method::PANE_EXPERIMENT_DELETE {
            return self.delete_experiment_capture_data(connection, params);
        }
        let id = errors::param_str(params, "paneId")?;
        if method == method::PANE_CLOSE && self.agents.contains_key(id) {
            return self.close_agent(connection, params);
        }
        if method == method::PANE_CLOSE {
            self.settle_editor_pane_before_close(id);
        }
        let mut saved = self
            .panes
            .get(id)
            .filter(|p| !p.stored.closed)
            .ok_or_else(|| errors::not_found("Pane not found"))?
            .stored
            .clone();
        let mut layout_workspace = None;
        let requested_capture = (method == method::PANE_EXPERIMENT_SET)
            .then(|| errors::param_bool(params, "on"))
            .transpose()?;
        let capture_is_active =
            self.terminal_captures.contains_key(id) || self.agent_captures.contains_key(id);
        let prepared_capture = if requested_capture == Some(true) && !capture_is_active {
            Some(match saved.pane.mode {
                PaneMode::Terminal => {
                    PreparedExperimentCapture::Terminal(self.prepare_terminal_capture(id)?)
                }
                PaneMode::Agent => {
                    PreparedExperimentCapture::Agent(self.prepare_agent_capture(id)?)
                }
                _ => {
                    return Err(errors::invalid(
                        "Experiment capture requires a terminal or agent pane",
                    ));
                }
            })
        } else {
            None
        };
        let mut events = vec![];
        let result = match method {
            method::PANE_CLOSE => {
                layout_workspace = self.prepare_pane_close(&mut saved, &mut events)?;
                json!({"paneId":id})
            }
            method::PANE_RENAME => {
                saved.pane.name = errors::param_str(params, "name")?.into();
                events.push((event::PANE_RENAMED.into(), json!({"pane":saved.pane})));
                json!({"pane":saved.pane})
            }
            method::PANE_RESIZE => {
                saved.pane.cols = u32::try_from(errors::param_u64(params, "cols")?)
                    .map_err(|_| errors::invalid("Pane columns exceed the supported range"))?;
                saved.pane.rows = u32::try_from(errors::param_u64(params, "rows")?)
                    .map_err(|_| errors::invalid("Pane rows exceed the supported range"))?;
                json!({"cols":saved.pane.cols,"rows":saved.pane.rows})
            }
            method::PANE_RECORD_SET => {
                if saved.pane.mode == soyeht_protocol::PaneMode::Agent {
                    return Err(errors::invalid("Agent protocol history is always durable"));
                }
                saved.pane.record = errors::param_bool(params, "on")?;
                json!({"on":saved.pane.record})
            }
            method::PANE_EXPERIMENT_SET => {
                if !matches!(saved.pane.mode, PaneMode::Terminal | PaneMode::Agent) {
                    return Err(errors::invalid(
                        "Experiment capture requires a terminal or agent pane",
                    ));
                }
                saved.pane.experiment_capture = Some(PaneExperimentCapture {
                    on: errors::invariant(
                        requested_capture,
                        "Experiment capture request disappeared",
                    )?,
                    failure: None,
                });
                json!({"capture":saved.pane.experiment_capture})
            }
            method::PANE_FOCUS => {
                let scope =
                    errors::invariant(self.clients.get(connection), "Pane connection disappeared")?
                        .scope
                        .clone();
                return Ok(self
                    .mutate(connection, method, params, |tx| {
                        tx.focus_pane(&scope, &saved.pane.workspace_id, id)?;
                        Ok((json!({"paneId":id}), vec![]))
                    })?
                    .result);
            }
            _ => {
                return Err(errors::invalid("Unknown pane mutation"));
            }
        };
        if matches!(
            method,
            method::PANE_RESIZE | method::PANE_RECORD_SET | method::PANE_EXPERIMENT_SET
        ) {
            events.push(model::pane_updated(&saved.pane)?);
        }
        let closes_editor = saved.closed && saved.pane.mode == PaneMode::Editor;
        let mutation = self.mutate(connection, method, params, |tx| {
            persist_pane(tx, &saved)?;
            if let Some(workspace) = &layout_workspace {
                tx.update_workspace(workspace)?;
            }
            if closes_editor {
                events.push(crate::editor::refresh_tabs(tx, &saved.pane.workspace_id)?);
            }
            Ok((result, events))
        })?;
        if !mutation.repeated {
            self.apply_committed_pane_edit(id, method, saved, prepared_capture)?;
        }
        Ok(mutation.result)
    }

    fn apply_committed_pane_edit(
        &mut self,
        id: &str,
        method: &str,
        saved: StoredPane,
        prepared_capture: Option<PreparedExperimentCapture>,
    ) -> Result<()> {
        let running = errors::invariant(
            self.panes.get_mut(id),
            "Pane disappeared after its mutation committed",
        )?;
        running.screen.recording = saved.pane.record;
        running.stored = saved.clone();
        if method == method::PANE_EXPERIMENT_SET {
            if saved
                .pane
                .experiment_capture
                .as_ref()
                .is_some_and(|capture| capture.on)
            {
                if let Some(capture) = prepared_capture {
                    match capture {
                        PreparedExperimentCapture::Terminal(capture) => {
                            self.terminal_captures.insert(id.into(), capture);
                        }
                        PreparedExperimentCapture::Agent(capture) => {
                            self.agent_captures.insert(id.into(), capture);
                            self.record_agent_label(id)?;
                        }
                    }
                }
            } else {
                self.terminal_captures.remove(id);
                self.agent_captures.remove(id);
            }
        }
        if saved.closed {
            self.finish_close(id)?;
        }
        if method == method::PANE_RESIZE && saved.pane.mode == PaneMode::Terminal {
            self.resize_terminal_capture(id)?;
            if let Some(instance_id) = saved.instance_id {
                // Best effort: the size is already durable, and a lost supervisor is
                // recorded by pty_request itself.
                let _ = self.pty_request(Request::Resize {
                    instance_id,
                    cols: saved.pane.cols as u16,
                    rows: saved.pane.rows as u16,
                });
            }
        }
        Ok(())
    }

    pub fn finish_close(&mut self, id: &str) -> Result<()> {
        self.editor.remove_pane(id);
        self.terminal_captures.remove(id);
        self.agent_captures.remove(id);
        for client in self.clients.values_mut() {
            client.streams.remove(id);
        }
        let saved = &errors::invariant(self.panes.get(id), "Closing pane disappeared")?.stored;
        if matches!(
            saved.pane.mode,
            PaneMode::Editor | PaneMode::Web | PaneMode::Writing
        ) {
            return Ok(());
        }
        let mut instance_id = saved.instance_id.clone();
        let key = saved.instance_key.clone();
        if instance_id.is_none()
            && let Ok(soyeht_ptyd::wire::Response::Instances(instances)) =
                self.pty_request(Request::List)
        {
            instance_id = instances
                .into_iter()
                .find(|i| i.spawn.instance_key == key)
                .map(|i| i.instance_id);
        }
        if let Some(instance_id) = instance_id
            && self.pty_request(Request::Close { instance_id }).is_err()
        {
            self.ptyd = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::testing::MemoryStore;

    #[test]
    fn pane_rule_reads_defaults_through_the_store_port() {
        let mut store = MemoryStore::default();
        store
            .settings
            .insert(setting::TERMINAL_DEFAULT_COLS.into(), json!(132));
        store
            .settings
            .insert(setting::TERMINAL_DEFAULT_ROWS.into(), json!(44));
        store
            .settings
            .insert(setting::RECORD_DEFAULT.into(), json!(false));

        assert_eq!(
            pane_defaults(&store).unwrap(),
            PaneDefaults {
                cols: 132,
                rows: 44,
                record: false,
            }
        );
    }
}
