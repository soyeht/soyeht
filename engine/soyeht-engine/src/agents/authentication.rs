use super::{
    Changes, registry, responses,
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
use soyeht_protocol::{HarnessLaunch, PaneMode, PaneState, StateSource};
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
            Some(package) if registry::pinned_package(package) => vec![package.clone()],
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
    agent_id: &str,
    child_id: &str,
    pane: &soyeht_protocol::Pane,
) -> (Value, Vec<(String, Value)>) {
    (
        json!({"paneId":agent_id,"terminalPaneId":child_id}),
        vec![("pane.created".into(), json!({"pane":pane}))],
    )
}

fn embedded_auth(state: &mut super::state::State, method_id: &str, now_ms: i64) -> Result<Changes> {
    let mut changes = Changes::default();
    responses::call(
        state,
        &mut changes,
        "authenticate",
        json!({"methodId":method_id}),
        "auth",
        now_ms,
    )?;
    Ok(changes)
}

impl Core {
    pub fn agent_auth_start(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let id = errors::param_str(params, "paneId")?;
        let mut state = self.agent_state(id)?;
        if !state.auth_required && state.phase != Phase::NeedsLogin {
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
                    Some((connection, "pane.auth.start", params)),
                    result,
                )?
                .result);
        }
        if state.client.pending.values().any(|c| c.context == "auth") {
            return Err(errors::pane_busy("Sign-in is already pending"));
        }
        let methods = &state
            .initialized
            .as_ref()
            .ok_or_else(|| errors::pane_busy("Agent has not initialized"))?
            .auth_methods;
        let method = methods
            .iter()
            .find(|m| {
                params["authMethodId"]
                    .as_str()
                    .is_none_or(|id| m.public.id == id)
            })
            .cloned()
            .ok_or_else(|| errors::invalid("Authentication method is not offered"))?;
        if method.kind == AuthKind::Agent {
            let changes = embedded_auth(&mut state, &method.public.id, self.now_ms())?;
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
            return Err(errors::invalid("Unsupported authentication method"));
        }
        // The adapter supplied auth args, but only the catalogue supplies the
        // runner and pinned package. Refuse a stale or replaced launch before
        // creating a terminal pane or spawning any process.
        let argv = verified_terminal_auth_argv(&self.directory, &state, method.args.clone())?;
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
        let mut environment =
            super::environment::launch_environment_with(state.launch.environment.as_deref());
        environment.extend(method.env);
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
        let screen = Screen::open(&self.directory.join("sessions").join(&child_id), false)?;
        state.login_pane_id = Some(child_id.clone());
        let generation = self.panes[id].stored.pane.generation;
        let mutation = self.mutate(connection, "pane.auth.start", params, |tx| {
            tx.insert_pane(&child)?;
            tx.insert_pane_launch(&child_id, &launch)?;
            soyeht_store::agents::save_runtime(tx, id, generation, &state)?;
            Ok(auth_pane_created(id, &child_id, &child.pane))
        })?;
        if !mutation.repeated {
            self.replace_agent_state(id, state)?;
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
            return Err(errors::internal("Sign-in pane is missing"));
        };
        if !child.closed && child.pane.state != PaneState::Dead {
            return Ok(());
        }
        let Response::Instances(instances) = self.pty_request(Request::List)? else {
            return Err(errors::ptyd_unavailable("Invalid sign-in process registry"));
        };
        let success = !child.closed
            && instances
                .iter()
                .find(|i| i.spawn.instance_key == child.instance_key)
                .is_some_and(|i| i.exit_code == Some(0));
        state.login_pane_id = None;
        state.auth_required = !success;
        state.phase = if success {
            Phase::RestartAfterProbe
        } else {
            Phase::NeedsLogin
        };
        state.last_update_ms = self.now_ms();
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
            &launch("/usr/local/bin/uv", &[package, "--acp"]),
            "uvx",
            vec!["--package".into(), "evil".into()],
        )
        .unwrap();
        assert_eq!(argv, ["/usr/local/bin/uv", package, "--package", "evil"]);
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
            ("/usr/local/bin/uv", "uvx", vec!["--acp"]),
            ("/usr/local/bin/uv", "uvx", vec!["--from=evil==1"]),
        ] {
            assert!(
                terminal_auth_argv(&launch(command, &args), original, vec!["login".into()])
                    .is_err()
            );
        }
    }
}
