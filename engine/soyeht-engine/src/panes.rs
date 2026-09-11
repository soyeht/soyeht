use crate::{
    core::{Core, write_pane},
    errors::{self, Result},
    model::{self, RunningPane, StoredPane},
    screen::Screen,
};
use rusqlite::params;
use serde_json::{Value, json};
use soyeht_protocol::{ErrorKind, Pane, PaneMode, PaneState, StateSource};
use soyeht_ptyd::wire::Request;
use std::path::Path;
use uuid::Uuid;

impl Core {
    pub fn pane_create(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let agent_mode = params["mode"] == "agent";
        let mut agent = if agent_mode {
            let harness = params["harness"].as_str().ok_or_else(|| {
                errors::error(ErrorKind::InvalidParams, "Agent panes require a harness")
            })?;
            let (launch, digest) = crate::agents::registry::prepare(&self.directory, harness)?;
            let permission = params
                .get("permission")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| errors::error(ErrorKind::InvalidParams, "Invalid permission mode"))?
                .unwrap_or(soyeht_protocol::PermissionMode::Ask);
            Some(crate::agents::state::State::new(launch, digest, permission))
        } else {
            None
        };
        if let Some(state) = &mut agent {
            for name in ["model", "effort"] {
                if let Some(value) = params[name].as_str() {
                    state
                        .requested_options
                        .insert(name.into(), soyeht_protocol::ConfigValue::V0(value.into()));
                }
            }
        }
        let workspace = model::workspace(
            &self.store,
            params["workspaceId"].as_str().expect("workspace"),
        )?;
        let cwd = params["cwd"].as_str().unwrap_or(&workspace.path);
        if !Path::new(cwd).is_absolute() || !Path::new(cwd).is_dir() {
            return Err(errors::error(
                ErrorKind::InvalidParams,
                "Pane directory must exist and be absolute",
            ));
        }
        let canonical;
        let cwd = if agent_mode {
            canonical = Path::new(cwd).canonicalize().map_err(errors::io)?;
            canonical.to_str().ok_or_else(|| {
                errors::error(ErrorKind::InvalidParams, "Working directory must be UTF-8")
            })?
        } else {
            cwd
        };
        let pane = Pane {
            agent: agent
                .as_ref()
                .map(|agent| agent.public(crate::agents::now_ms())),
            pane_id: Uuid::new_v4().to_string(),
            workspace_id: workspace.workspace_id,
            cwd: cwd.into(),
            name: params["name"]
                .as_str()
                .unwrap_or(if agent_mode { "Agent" } else { "Shell" })
                .into(),
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
            cols: params["cols"].as_u64().unwrap_or(
                self.setting("terminal.defaultCols")?
                    .as_u64()
                    .expect("column setting"),
            ) as u32,
            rows: params["rows"].as_u64().unwrap_or(
                self.setting("terminal.defaultRows")?
                    .as_u64()
                    .expect("row setting"),
            ) as u32,
            record: agent_mode
                || self
                    .setting("record.default")?
                    .as_bool()
                    .expect("boolean setting"),
            pid: None,
            stream_id: (!agent_mode).then(|| Uuid::new_v4().to_string()),
        };
        let saved = StoredPane {
            pane,
            closed: false,
            instance_key: Uuid::new_v4().to_string(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        };
        // Open storage before accepting a process creation. No child exists yet.
        let screen = Screen::open(
            &self.directory.join("sessions").join(&saved.pane.pane_id),
            saved.pane.record,
        )
        .map_err(errors::io)?;
        let mutation=self.mutate(connection,"pane.create",params,|tx| {
            tx.execute("INSERT INTO panes(id,workspace_id,payload,instance_key,sequence_base) VALUES (?1,?2,?3,?4,'0')",params![saved.pane.pane_id,saved.pane.workspace_id,serde_json::to_string(&saved.pane)?,saved.instance_key])?;
            if let Some(agent)=&agent {soyeht_store::agents::save_runtime(tx,&saved.pane.pane_id,saved.pane.generation,agent)?;}
            Ok((json!({"paneId":saved.pane.pane_id}),vec![("pane.created".into(),json!({"pane":saved.pane}))]))
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
            // A disconnected supervisor leaves a durable creation intent for
            // reconciliation, never a second process on an RPC retry.
            if self.spawn(&id).is_err() {
                self.ptyd = None;
            }
        }
        Ok(mutation.result)
    }

    pub fn pane_edit(&mut self, connection: &str, method: &str, params: &Value) -> Result<Value> {
        let id = params["paneId"].as_str().expect("pane");
        if method == "pane.close" && self.agents.contains_key(id) {
            return self.close_agent(connection, params);
        }
        let mut saved = self
            .panes
            .get(id)
            .filter(|p| !p.stored.closed)
            .ok_or_else(|| errors::error(ErrorKind::NotFound, "Pane not found"))?
            .stored
            .clone();
        let mut events = vec![];
        let result = match method {
            "pane.close" => {
                saved.closed = true;
                events.push((
                    "pane.closed".into(),
                    json!({"paneId":id,"workspaceId":saved.pane.workspace_id}),
                ));
                json!({"paneId":id})
            }
            "pane.rename" => {
                saved.pane.name = params["name"].as_str().expect("name").into();
                events.push(("pane.renamed".into(), json!({"pane":saved.pane})));
                json!({"pane":saved.pane})
            }
            "pane.resize" => {
                saved.pane.cols = params["cols"].as_u64().expect("cols") as u32;
                saved.pane.rows = params["rows"].as_u64().expect("rows") as u32;
                json!({"cols":saved.pane.cols,"rows":saved.pane.rows})
            }
            "pane.record.set" => {
                if saved.pane.mode == soyeht_protocol::PaneMode::Agent {
                    return Err(errors::error(
                        ErrorKind::InvalidParams,
                        "Agent protocol history is always durable",
                    ));
                }
                saved.pane.record = params["on"].as_bool().expect("record");
                json!({"on":saved.pane.record})
            }
            "pane.focus" => {
                let scope = self.clients[connection].scope.clone();
                return Ok(self.mutate(connection,method,params,|tx| {
                    tx.execute("INSERT INTO client_focus(scope,workspace_id,pane_id) VALUES (?1,?2,?3) ON CONFLICT(scope) DO UPDATE SET workspace_id=excluded.workspace_id,pane_id=excluded.pane_id",params![scope,saved.pane.workspace_id,id])?;
                    Ok((json!({"paneId":id}),vec![]))
                })?.result);
            }
            _ => {
                return Err(errors::error(
                    ErrorKind::InvalidParams,
                    "Unknown pane mutation",
                ));
            }
        };
        if matches!(method, "pane.resize" | "pane.record.set") {
            events.push(("pane.updated".into(), json!({"pane":saved.pane})));
        }
        let mutation = self.mutate(connection, method, params, |tx| {
            write_pane(tx, &saved)?;
            if saved.closed {
                tx.execute(
                    "UPDATE client_focus SET pane_id=NULL WHERE pane_id=?1",
                    [id],
                )?;
            }
            Ok((result, events))
        })?;
        if !mutation.repeated {
            let running = self.panes.get_mut(id).expect("pane");
            running.screen.recording = saved.pane.record;
            running.stored = saved.clone();
            if saved.closed {
                self.finish_close(id);
            }
            if method == "pane.resize"
                && saved.pane.mode == PaneMode::Terminal
                && let Some(instance_id) = saved.instance_id
            {
                let _ = self.pty_request(Request::Resize {
                    instance_id,
                    cols: saved.pane.cols as u16,
                    rows: saved.pane.rows as u16,
                });
            }
        }
        Ok(mutation.result)
    }

    pub fn finish_close(&mut self, id: &str) {
        for client in self.clients.values_mut() {
            client.streams.remove(id);
        }
        let saved = &self.panes[id].stored;
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
    }
}
