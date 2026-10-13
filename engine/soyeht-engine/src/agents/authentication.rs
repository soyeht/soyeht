use super::{
    Changes,
    purpose::Purpose,
    registry, responses,
    state::{Phase, State},
};
use crate::{
    core::Core,
    errors::{self, Result},
    model::{RunningPane, StoredPane},
    screen::Screen,
};
use serde_json::{Value, json};
use soyeht_acp::session::AuthKind;
use soyeht_protocol::{HarnessLaunch, PaneMode, PaneState, StateSource, event, method};
use soyeht_ptyd::wire::{ProcessKind, Request, Response, Spawn};
use std::path::Path;
use uuid::Uuid;

fn terminal_auth_argv(
    launch: &HarnessLaunch,
    original_command: &str,
    auth_args: Vec<String>,
) -> Result<Vec<String>> {
    let runner = Path::new(original_command)
        .file_name()
        .and_then(|name| name.to_str());
    let package_prefix = match runner {
        Some("npx" | "npx-cli.js") => match launch.args.as_slice() {
            [yes, package, ..] if yes == "--yes" && registry::pinned_package(package) => {
                vec![yes.clone(), package.clone()]
            }
            _ => {
                return Err(errors::invalid(
                    "Terminal sign-in requires a version-pinned harness package",
                ));
            }
        },
        Some("uvx") => match launch.args.first() {
            Some(package) if registry::pinned_package(package) => match Path::new(&launch.command)
                .file_name()
                .and_then(|name| name.to_str())
            {
                Some("uvx") => vec![package.clone()],
                Some("uv") => vec!["tool".into(), "run".into(), package.clone()],
                _ => {
                    return Err(errors::invalid(
                        "Terminal sign-in requires a recognized uv runner",
                    ));
                }
            },
            _ => {
                return Err(errors::invalid(
                    "Terminal sign-in requires a version-pinned harness package",
                ));
            }
        },
        _ => vec![],
    };
    // npx and uvx pass arguments after the package position to that package's
    // executable. In particular, an authMethod --package must never be a runner option.
    Ok(std::iter::once(launch.command.clone())
        .chain(package_prefix)
        .chain(auth_args)
        .collect())
}

fn verified_terminal_auth_argv(
    directory: &Path,
    state: &State,
    auth_args: Vec<String>,
) -> Result<Vec<String>> {
    let entry = registry::catalogue_entries(directory)?
        .into_iter()
        .find(|entry| entry.launch.id == state.launch.id)
        .ok_or_else(|| errors::invalid("Harness changed; restart the agent pane"))?;
    let (current, digest) = registry::prepare_entry(entry.clone())?;
    if current != state.launch || digest != state.binary_digest {
        return Err(errors::invalid("Harness changed; restart the agent pane"));
    }
    terminal_auth_argv(&state.launch, &entry.launch.command, auth_args)
}

fn auth_pane_created(
    agent: &soyeht_protocol::Pane,
    child: &soyeht_protocol::Pane,
) -> (Value, Vec<(String, Value)>) {
    (
        json!({"paneId":agent.pane_id,"terminalPaneId":child.pane_id}),
        vec![
            (event::PANE_CREATED.into(), json!({"pane":child})),
            (event::PANE_UPDATED.into(), json!({"pane":agent})),
        ],
    )
}

fn embedded_auth(state: &mut super::state::State, method_id: &str, now_ms: i64) -> Result<Changes> {
    let mut changes = Changes::default();
    responses::call(
        state,
        &mut changes,
        "authenticate",
        json!({"methodId":method_id}),
        &Purpose::Authenticate,
        now_ms,
    )?;
    Ok(changes)
}

/// The terminal pane that runs a sign-in beside its agent, and the process it spawns.
fn login_terminal(
    agent: &soyeht_protocol::Pane,
    state: &State,
    argv: Vec<String>,
    method_env: std::collections::BTreeMap<String, String>,
) -> (StoredPane, Spawn) {
    let mut pane = agent.clone();
    pane.pane_id = Uuid::new_v4().to_string();
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
    let mut environment =
        super::environment::launch_environment_with(state.launch.environment.as_deref());
    environment.extend(method_env);
    let launch = Spawn {
        instance_key: child.instance_key.clone(),
        generation: 1,
        kind: ProcessKind::Pty,
        cwd: child.pane.cwd.clone(),
        argv,
        env: environment,
        cols: child.pane.cols as u16,
        rows: child.pane.rows as u16,
    };
    (child, launch)
}

impl Core {
    /// `pane.auth.start`, `pane.auth.cancel` and `pane.auth.logout`.
    pub fn agent_auth(&mut self, connection: &str, method: &str, params: &Value) -> Result<Value> {
        match method {
            method::PANE_AUTH_CANCEL => self.agent_auth_cancel(connection, params),
            method::PANE_AUTH_LOGOUT => self.agent_auth_logout(connection, params),
            _ => self.agent_auth_start(connection, params),
        }
    }

    pub fn agent_auth_start(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let id = errors::param_str(params, "paneId")?;
        let mut state = self.agent_state(id)?;
        let relogin = state.is_signed_in();
        if !state.auth_required && state.phase != Phase::NeedsLogin && !relogin {
            return Err(errors::invalid("Sign-in is not required for this pane"));
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
                    Some((connection, method::PANE_AUTH_START, params)),
                    result,
                )?
                .result);
        }
        if state
            .client
            .pending
            .values()
            .any(|c| Purpose::parse(&c.context) == Purpose::Authenticate)
        {
            return Err(errors::pane_busy("Sign-in is already pending"));
        }
        if relogin {
            state.account_busy()?;
        }
        let methods = &state
            .initialized
            .as_ref()
            .ok_or_else(|| errors::pane_busy("Agent has not initialized"))?
            .auth_methods;
        let method = methods
            .iter()
            .find(|m| {
                // A working session is only ever re-signed in through a terminal.
                (!relogin || m.kind == AuthKind::Terminal)
                    && params["authMethodId"]
                        .as_str()
                        .is_none_or(|id| m.public.id == id)
            })
            .cloned()
            .ok_or_else(|| errors::invalid("Authentication method is not offered"))?;
        if method.kind == AuthKind::Agent && !relogin {
            let changes = embedded_auth(&mut state, &method.public.id, self.now_ms())?;
            return Ok(self
                .commit_agent(
                    self.panes[id].stored.clone(),
                    state,
                    changes,
                    Some((connection, method::PANE_AUTH_START, params)),
                    json!({"paneId":id}),
                )?
                .result);
        }
        if method.kind != AuthKind::Terminal {
            return Err(errors::invalid("Unsupported authentication method"));
        }
        // The adapter supplied auth args, but only the catalogue supplies the
        // runner and pinned package. Refuse a stale or replaced launch before
        // creating a terminal pane or spawning any process.
        let argv = verified_terminal_auth_argv(&self.directory, &state, method.args.clone())?;
        let (child, launch) = login_terminal(&self.panes[id].stored.pane, &state, argv, method.env);
        let child_id = child.pane.pane_id.clone();
        let screen = Screen::open(&self.directory.join("sessions").join(&child_id), false)?;
        state.login_pane_id = Some(child_id.clone());
        state.relogin = relogin;
        // The agent pane publishes the running sign-in with the terminal's creation.
        let mut saved = self.panes[id].stored.clone();
        saved.pane.agent = Some(state.public(self.now_ms()));
        let generation = saved.pane.generation;
        let mutation = self.mutate(connection, method::PANE_AUTH_START, params, |tx| {
            tx.insert_pane(&child)?;
            tx.insert_pane_launch(&child_id, &launch)?;
            crate::core::write_pane(tx, &saved)?;
            soyeht_store::agents::save_runtime(tx, id, generation, &state)?;
            Ok(auth_pane_created(&saved.pane, &child.pane))
        })?;
        if !mutation.repeated {
            self.replace_agent_state(id, state)?;
            errors::invariant(self.panes.get_mut(id), "Agent pane disappeared")?.stored = saved;
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
        let state = self.agent_state(id)?;
        if state.login_pane_id.is_some() {
            self.finish_login_terminal(id, state)
        } else if state.unconfirmed_login_pane_id.is_some() {
            self.confirm_login_terminal(id, state)
        } else {
            Ok(())
        }
    }

    /// A zero exit status is only a claim that sign-in worked. The agent
    /// restarts to check it, and the terminal stays open until that check ends.
    fn finish_login_terminal(&mut self, id: &str, mut state: State) -> Result<()> {
        let Some(child_id) = state.login_pane_id.clone() else {
            return Ok(());
        };
        let Some(child) = self.panes.get(&child_id).map(|p| p.stored.clone()) else {
            return Err(errors::internal("Sign-in pane is missing"));
        };
        if !child.closed && child.pane.state != PaneState::Dead {
            return Ok(());
        }
        if state.relogin && (state.active.is_some() || state.operation.is_some()) {
            // The restart that picks up the new account waits for the turn.
            return Ok(());
        }
        let Response::Instances(instances) = self.pty_request(Request::List)? else {
            return Err(errors::ptyd_unavailable("Invalid sign-in process registry"));
        };
        let exit_code = instances
            .iter()
            .find(|i| i.spawn.instance_key == child.instance_key)
            .and_then(|i| i.exit_code);
        let exited_cleanly = !child.closed && exit_code == Some(0);
        // The only durable account of why a sign-in process ended.
        eprintln!(
            "{}",
            json!({
                "event": "engine.sign_in_ended",
                "paneId": id,
                "terminalPaneId": child_id,
                "closedByPerson": child.closed,
                "processLost": child.pane.phrase == SIGN_IN_PROCESS_LOST,
                "exitCode": exit_code,
            })
        );
        state.login_pane_id = None;
        let relogin = std::mem::take(&mut state.relogin);
        state.last_update_ms = self.now_ms();
        let mut changes = Changes::default();
        if exited_cleanly {
            state.unconfirmed_login_pane_id = Some(child_id);
            state.auth_required = false;
            state.phase = Phase::RestartAfterProbe;
            changes.clear_failed_probes = true;
        } else {
            if !relogin {
                state.auth_required = true;
                if state.phase != Phase::Faulted {
                    state.phase = Phase::NeedsLogin;
                }
            }
            changes
                .events
                .push(auth_finished(id, false, unfinished_reason(&child)));
        }
        self.commit_agent(
            self.panes[id].stored.clone(),
            state,
            changes,
            None,
            json!({}),
        )?;
        Ok(())
    }

    /// Closes the sign-in terminal only once the restarted agent has a
    /// session. If the agent still asks for sign-in, the terminal and its
    /// output stay available for review.
    fn confirm_login_terminal(&mut self, id: &str, mut state: State) -> Result<()> {
        let signed_in =
            state.phase == Phase::Ready && !state.auth_required && state.session_id.is_some();
        let reason = match state.phase {
            _ if signed_in => "Sign-in completed",
            Phase::NeedsLogin => {
                "Sign-in did not complete. Review its terminal, then sign in again."
            }
            Phase::Faulted => "The agent stopped before sign-in could be confirmed.",
            _ => return Ok(()),
        };
        let Some(child_id) = state.unconfirmed_login_pane_id.take() else {
            return Ok(());
        };
        state.last_update_ms = self.now_ms();
        let mut changes = Changes::default();
        if signed_in
            && let Some(mut child) = self
                .panes
                .get(&child_id)
                .map(|p| p.stored.clone())
                .filter(|p| !p.closed)
        {
            changes.workspace_layout = self.prepare_pane_close(&mut child, &mut changes.events)?;
            changes.closed_login_pane = Some(child);
        }
        changes.events.push(auth_finished(id, signed_in, reason));
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

/// The phrase of a sign-in terminal whose process the supervisor no longer has.
pub(crate) const SIGN_IN_PROCESS_LOST: &str = "Sign-in process was lost";

/// What the person is told when a sign-in ends without signing in.
fn unfinished_reason(terminal: &StoredPane) -> &'static str {
    if terminal.closed {
        "The sign-in terminal was closed before sign-in finished. Sign in again."
    } else if terminal.pane.phrase == SIGN_IN_PROCESS_LOST {
        "Sign-in stopped because Soyeht's background services restarted. Sign in again."
    } else {
        "Sign-in failed or was interrupted. Review its terminal, then sign in again."
    }
}

pub(super) fn auth_finished(id: &str, ok: bool, reason: &str) -> (String, Value) {
    (
        event::PANE_AUTH_FINISHED.into(),
        json!({"paneId":id,"ok":ok,"reason":reason}),
    )
}

#[cfg(test)]
mod tests {
    use super::terminal_auth_argv;
    use soyeht_protocol::HarnessLaunch;

    fn launch(command: &str, args: &[&str]) -> HarnessLaunch {
        HarnessLaunch {
            id: "agent".into(),
            name: "Agent".into(),
            command: command.into(),
            args: args.iter().map(|arg| (*arg).into()).collect(),
            version: "1".into(),
            environment: None,
        }
    }

    #[test]
    fn terminal_authentication_replaces_the_acp_arguments() {
        let argv = terminal_auth_argv(
            &launch("/agent", &["--acp"]),
            "agent",
            vec!["--cli".into(), "auth".into(), "login".into()],
        )
        .unwrap();
        assert_eq!(argv, ["/agent", "--cli", "auth", "login"]);
        assert!(!argv.contains(&"--acp".into()));
    }

    #[test]
    fn npx_authentication_keeps_the_launch_package_before_untrusted_method_args() {
        let package = "@agentclientprotocol/claude-agent-acp@0.77.0";
        let argv = terminal_auth_argv(
            &launch("/node/bin/npx-cli.js", &["--yes", package, "--acp"]),
            "npx",
            vec!["--package".into(), "evil".into(), "x".into()],
        )
        .unwrap();
        assert_eq!(
            argv,
            [
                "/node/bin/npx-cli.js",
                "--yes",
                package,
                "--package",
                "evil",
                "x"
            ]
        );
    }

    #[test]
    fn uvx_authentication_keeps_the_launch_package_before_method_args() {
        let package = "fast-agent-acp==0.10.1";
        let argv = terminal_auth_argv(
            &launch("/usr/local/bin/uvx", &[package, "--acp"]),
            "uvx",
            vec!["--package".into(), "evil".into()],
        )
        .unwrap();
        assert_eq!(argv, ["/usr/local/bin/uvx", package, "--package", "evil"]);
    }

    #[test]
    fn uvx_resolved_to_uv_uses_tool_run_before_the_pinned_package() {
        let package = "fast-agent-acp==0.10.1";
        let argv = terminal_auth_argv(
            &launch("/usr/local/bin/uv", &[package, "--acp"]),
            "uvx",
            vec!["--package".into(), "evil".into()],
        )
        .unwrap();
        assert_eq!(
            argv,
            [
                "/usr/local/bin/uv",
                "tool",
                "run",
                package,
                "--package",
                "evil"
            ]
        );
    }

    #[test]
    fn package_runners_refuse_sign_in_without_the_pinned_launch_package() {
        for (command, original, args) in [
            ("/node/bin/npx-cli.js", "npx", vec!["--yes", "--acp"]),
            (
                "/node/bin/npx-cli.js",
                "npx",
                vec!["--yes", "--package=evil@1"],
            ),
            ("/usr/local/bin/uvx", "uvx", vec!["--acp"]),
            ("/usr/local/bin/uvx", "uvx", vec!["--from=evil==1"]),
        ] {
            assert!(
                terminal_auth_argv(&launch(command, &args), original, vec!["login".into()])
                    .is_err()
            );
        }
    }
}
