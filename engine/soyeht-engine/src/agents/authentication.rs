use super::{Changes, now_ms, responses, state::Phase};
use crate::{
    core::Core,
    errors::{self, Result},
    model::{RunningPane, StoredPane},
    screen::Screen,
};
use serde_json::{Value, json};
use soyeht_acp::session::AuthKind;
use soyeht_protocol::{ErrorKind, PaneMode, PaneState, StateSource};
use soyeht_ptyd::wire::{ProcessKind, Request, Response, Spawn};
use uuid::Uuid;

fn auth_pane_created(
    agent_id: &str,
    child_id: &str,
    pane: &soyeht_protocol::Pane,
) -> (Value, Vec<(String, Value)>) {
    (
        json!({"paneId":agent_id,"terminalPaneId":child_id}),
        vec![("pane.created".into(), json!({"pane":pane}))],
    )
}

impl Core {
    pub fn agent_auth_start(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let id = params["paneId"].as_str().expect("validated pane");
        let mut state = self.agent_state(id)?;
        if state.phase != Phase::NeedsLogin {
            return Err(errors::error(
                ErrorKind::PaneBusy,
                "This pane is not waiting for sign-in",
            ));
        }
        if let Some(pane) = state.login_pane_id.as_ref().filter(|pane| {
            self.panes
                .get(*pane)
                .is_some_and(|p| !p.stored.closed && p.stored.pane.state != PaneState::Dead)
        }) {
            let result = json!({"paneId":id,"terminalPaneId":pane});
            return Ok(self
                .commit_agent(
                    self.panes[id].stored.clone(),
                    state,
                    Changes::default(),
                    Some((connection, "pane.auth.start", params)),
                    result,
                )?
                .result);
        }
        if state.client.pending.values().any(|c| c.context == "auth") {
            return Err(errors::error(
                ErrorKind::PaneBusy,
                "Sign-in is already pending",
            ));
        }
        let methods = &state
            .initialized
            .as_ref()
            .ok_or_else(|| errors::error(ErrorKind::PaneBusy, "Agent has not initialized"))?
            .auth_methods;
        let method = methods
            .iter()
            .find(|m| {
                params["authMethodId"]
                    .as_str()
                    .is_none_or(|id| m.public.id == id)
            })
            .cloned()
            .ok_or_else(|| {
                errors::error(
                    ErrorKind::InvalidParams,
                    "Authentication method is not offered",
                )
            })?;
        if method.kind == AuthKind::Agent {
            let mut changes = Changes::default();
            responses::call(
                &mut state,
                &mut changes,
                "authenticate",
                json!({"methodId":method.public.id}),
                "auth",
            )?;
            return Ok(self
                .commit_agent(
                    self.panes[id].stored.clone(),
                    state,
                    changes,
                    Some((connection, "pane.auth.start", params)),
                    json!({"paneId":id}),
                )?
                .result);
        }
        if method.kind != AuthKind::Terminal {
            return Err(errors::error(
                ErrorKind::InvalidParams,
                "Unsupported authentication method",
            ));
        }
        let child_id = Uuid::new_v4().to_string();
        let mut pane = self.panes[id].stored.pane.clone();
        pane.pane_id = child_id.clone();
        pane.agent = None;
        pane.name = format!("Sign in — {}", state.launch.name);
        pane.mode = PaneMode::Terminal;
        pane.state = PaneState::Starting;
        pane.source = StateSource::None;
        pane.confidence = 0.0;
        pane.phrase = "Starting sign-in terminal".into();
        pane.generation = 1;
        pane.pid = None;
        pane.stream_id = Some(Uuid::new_v4().to_string());
        pane.record = false;
        let child = StoredPane {
            pane,
            closed: false,
            instance_key: Uuid::new_v4().to_string(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        };
        let mut argv = vec![state.launch.command.clone()];
        argv.extend(state.launch.args.clone());
        argv.extend(method.args);
        let launch = Spawn {
            instance_key: child.instance_key.clone(),
            generation: 1,
            kind: ProcessKind::Pty,
            cwd: child.pane.cwd.clone(),
            argv,
            env: method.env,
            cols: child.pane.cols as u16,
            rows: child.pane.rows as u16,
        };
        let screen = Screen::open(&self.directory.join("sessions").join(&child_id), false)
            .map_err(errors::io)?;
        state.login_pane_id = Some(child_id.clone());
        let generation = self.panes[id].stored.pane.generation;
        let mutation = self.mutate(connection, "pane.auth.start", params, |tx| {
            tx.insert_pane(&child)?;
            tx.insert_pane_launch(&child_id, &launch)?;
            soyeht_store::agents::save_runtime(tx, id, generation, &state)?;
            Ok(auth_pane_created(id, &child_id, &child.pane))
        })?;
        if !mutation.repeated {
            self.agents.get_mut(id).expect("agent").state = state;
            self.panes.insert(
                child_id.clone(),
                RunningPane {
                    stored: child,
                    screen,
                },
            );
            if self.spawn(&child_id).is_err() {
                self.ptyd = None;
            }
        }
        Ok(mutation.result)
    }

    pub fn poll_agent_authentication(&mut self, id: &str) -> Result<()> {
        let mut state = self.agent_state(id)?;
        let Some(child_id) = state.login_pane_id.clone() else {
            return Ok(());
        };
        let Some(child) = self.panes.get(&child_id).map(|p| p.stored.clone()) else {
            return Err(errors::error(
                ErrorKind::InternalError,
                "Sign-in pane is missing",
            ));
        };
        if !child.closed && child.pane.state != PaneState::Dead {
            return Ok(());
        }
        let Response::Instances(instances) = self.pty_request(Request::List)? else {
            return Err(errors::error(
                ErrorKind::PtydUnavailable,
                "Invalid sign-in process registry",
            ));
        };
        let success = !child.closed
            && instances
                .iter()
                .find(|i| i.spawn.instance_key == child.instance_key)
                .is_some_and(|i| i.exit_code == Some(0));
        state.login_pane_id = None;
        state.phase = if success {
            Phase::RestartAfterProbe
        } else {
            Phase::NeedsLogin
        };
        state.last_update_ms = now_ms();
        let mut changes = Changes {
            clear_failed_probes: success,
            ..Changes::default()
        };
        changes.events.push(("pane.auth_finished".into(),json!({"paneId":id,"ok":success,"reason":if success {"Sign-in completed"} else {"Sign-in failed or was interrupted"}})));
        self.commit_agent(
            self.panes[id].stored.clone(),
            state,
            changes,
            None,
            json!({}),
        )?;
        Ok(())
    }
}
