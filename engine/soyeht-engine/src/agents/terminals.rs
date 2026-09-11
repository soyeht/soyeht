use super::{Changes, effects, transcript};
use crate::{
    core::Core,
    errors::{self, Result},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::{ErrorKind, TerminalOutput, TranscriptKind, TranscriptRole};
use soyeht_ptyd::wire::{ProcessKind, Request, Response, Spawn};
use soyeht_store::agents::{self, NativeRequest};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Terminal {
    pub spawn: Spawn,
    pub instance_id: Option<String>,
    pub supervisor_id: Option<String>,
    pub bytes: String,
    pub from: u64,
    pub next: u64,
    pub limit: usize,
    pub truncated: bool,
    pub exit_code: Option<i32>,
    pub message_seq: Option<u64>,
}

impl Terminal {
    fn prepare(
        request: &NativeRequest,
        cwd: &str,
    ) -> std::result::Result<Self, soyeht_acp::RpcError> {
        let command = request.params["command"]
            .as_str()
            .filter(|s| !s.is_empty() && !s.contains('\0'))
            .ok_or_else(|| effects::failure("Terminal command is required"))?;
        let mut argv = vec![command.into()];
        if let Some(args) = request.params.get("args") {
            for arg in args
                .as_array()
                .ok_or_else(|| effects::failure("Terminal arguments must be an array"))?
            {
                argv.push(
                    arg.as_str()
                        .filter(|s| !s.contains('\0'))
                        .ok_or_else(|| effects::failure("Invalid terminal argument"))?
                        .into(),
                );
            }
        }
        let path = Path::new(request.params["cwd"].as_str().unwrap_or(cwd));
        if !path.is_absolute() {
            return Err(effects::failure("Terminal directory must be absolute"));
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| effects::failure(error.to_string()))?;
        if !canonical.is_dir() {
            return Err(effects::failure("Terminal directory does not exist"));
        }
        let mut env = BTreeMap::new();
        if let Some(values) = request.params.get("env") {
            for item in values
                .as_array()
                .ok_or_else(|| effects::failure("Terminal environment must be an array"))?
            {
                let name = item["name"]
                    .as_str()
                    .filter(|s| !s.is_empty() && !s.contains(['=', '\0']))
                    .ok_or_else(|| effects::failure("Invalid environment name"))?;
                let value = item["value"]
                    .as_str()
                    .filter(|s| !s.contains('\0'))
                    .ok_or_else(|| effects::failure("Invalid environment value"))?;
                if env.insert(name.into(), value.into()).is_some() {
                    return Err(effects::failure("Duplicate environment name"));
                }
            }
        }
        let limit = match request.params.get("outputByteLimit") {
            Some(value) if !value.is_null() => value
                .as_u64()
                .ok_or_else(|| effects::failure("Invalid terminal output limit"))?
                .min(64 * 1024) as usize,
            _ => 64 * 1024,
        };
        Ok(Self {
            spawn: Spawn {
                instance_key: format!("terminal:{}", request.request_id),
                generation: request.generation,
                kind: ProcessKind::Pty,
                cwd: canonical
                    .to_str()
                    .ok_or_else(|| effects::failure("Terminal directory is not UTF-8"))?
                    .into(),
                argv,
                env,
                cols: 100,
                rows: 30,
            },
            instance_id: None,
            supervisor_id: None,
            bytes: String::new(),
            from: 0,
            next: 0,
            limit,
            truncated: false,
            exit_code: None,
            message_seq: None,
        })
    }
    fn status(&self) -> Value {
        json!({"exitCode":self.exit_code.filter(|code| *code >= 0)})
    }
    fn output(&self) -> Value {
        let bytes = STANDARD.decode(&self.bytes).expect("stored terminal bytes");
        let mut output = String::from_utf8_lossy(&bytes).into_owned();
        let mut truncated = self.truncated;
        if output.len() > self.limit {
            let mut cut = output.len() - self.limit;
            while !output.is_char_boundary(cut) {
                cut += 1;
            }
            output.drain(..cut);
            truncated = true;
        }
        let mut result = json!({"output":output,"truncated":truncated});
        if self.exit_code.is_some() {
            result["exitStatus"] = self.status();
        }
        result
    }
}

impl Core {
    pub fn poll_agent_terminals(&mut self, pane: &str) -> Result<()> {
        let releasing: Vec<_> = agents::open_requests(self.store.connection(), pane)
            .map_err(errors::storage)?
            .into_iter()
            .filter(|r| r.method == "terminal/release")
            .filter_map(|r| r.params["terminalId"].as_str().map(str::to_owned))
            .collect();
        let ids: Vec<_> = self.agents[pane].state.terminals.keys().cloned().collect();
        for id in ids {
            if releasing.contains(&id) {
                continue;
            }
            let mut state = self.agent_state(pane)?;
            let saved = self.panes[pane].stored.clone();
            let mut terminal = state.terminals[&id].clone();
            let Some(instance_id) = terminal.instance_id.clone() else {
                continue;
            };
            if terminal.exit_code.is_some() {
                self.pty_request(Request::Ack {
                    instance_id,
                    seq: terminal.next,
                })?;
                continue;
            }
            let Response::Output(output) = self.pty_request(Request::Drain {
                instance_id: instance_id.clone(),
                from_seq: terminal.next,
            })?
            else {
                return Err(errors::error(
                    ErrorKind::PtydUnavailable,
                    "Invalid inline terminal output",
                ));
            };
            let incoming = STANDARD.decode(&output.bytes).map_err(|_| {
                errors::error(
                    ErrorKind::PtydUnavailable,
                    "Invalid terminal output encoding",
                )
            })?;
            if output.next_seq != output.from_seq + incoming.len() as u64 {
                return Err(errors::error(
                    ErrorKind::PtydUnavailable,
                    "Invalid terminal output offsets",
                ));
            }
            let mut bytes = STANDARD.decode(&terminal.bytes).map_err(|_| {
                errors::error(ErrorKind::InternalError, "Invalid stored terminal output")
            })?;
            let changed = !incoming.is_empty() || output.exited || output.from_seq != terminal.next;
            if output.from_seq != terminal.next {
                bytes.clear();
                terminal.from = output.from_seq;
                terminal.truncated = true;
            }
            bytes.extend(&incoming);
            if bytes.len() > terminal.limit {
                let mut cut = bytes.len() - terminal.limit;
                while cut < bytes.len() && bytes[cut] & 0xc0 == 0x80 {
                    cut += 1;
                }
                bytes.drain(..cut);
                terminal.from += cut as u64;
                terminal.truncated = true;
            }
            terminal.next = output.next_seq;
            terminal.bytes = STANDARD.encode(&bytes);
            if output.exited && output.next_seq == output.end_seq {
                let Response::Instances(instances) = self.pty_request(Request::List)? else {
                    return Err(errors::error(
                        ErrorKind::PtydUnavailable,
                        "Invalid process registry",
                    ));
                };
                terminal.exit_code = Some(
                    instances
                        .iter()
                        .find(|i| i.instance_id == instance_id)
                        .and_then(|i| i.exit_code)
                        .unwrap_or(-1),
                );
            }
            if !changed {
                self.pty_request(Request::Ack {
                    instance_id,
                    seq: terminal.next,
                })?;
                continue;
            }
            let mut changes = Changes {
                replay_messages: state.phase == super::state::Phase::Loading,
                ..Changes::default()
            };
            let prior = terminal
                .message_seq
                .map(|seq| transcript::message(self.store.connection(), pane, seq))
                .transpose()?
                .flatten();
            let mut item = match prior {
                Some(item) => item,
                None => transcript::new_message(
                    &mut state,
                    saved.pane.generation,
                    TranscriptRole::Assistant,
                    TranscriptKind::Terminal,
                    String::new(),
                    false,
                )?,
            };
            item.terminal = Some(TerminalOutput {
                terminal_id: id.clone(),
                bytes: terminal.bytes.clone(),
                offset: terminal.from.to_string(),
                truncated: terminal.truncated,
                exit_code: terminal.exit_code.map(i64::from),
            });
            item.complete = terminal.exit_code.is_some();
            terminal.message_seq = Some(item.seq.parse().expect("allocated sequence"));
            transcript::put(&mut changes, pane, item, "terminal_output")?;
            state.terminals.insert(id, terminal.clone());
            self.commit_agent(saved, state, changes, None, json!({}))?;
            self.pty_request(Request::Ack {
                instance_id,
                seq: terminal.next,
            })?;
        }
        Ok(())
    }
    pub fn run_native_terminals(&mut self, pane: &str) -> Result<()> {
        self.poll_agent_terminals(pane)?;
        let requests =
            agents::open_requests(self.store.connection(), pane).map_err(errors::storage)?;
        for request in requests
            .into_iter()
            .filter(|r| r.method.starts_with("terminal/"))
            .take(8)
        {
            let mut state = self.agent_state(pane)?;
            let saved = self.panes[pane].stored.clone();
            let mut changes = Changes::default();
            let result = if request.method == "terminal/create" {
                let id = request.request_id.clone();
                if !state.terminals.contains_key(&id) {
                    let prepared = if state.terminals.len() >= 16 {
                        Err(effects::failure("Inline terminal limit reached"))
                    } else {
                        Terminal::prepare(&request, &saved.pane.cwd)
                    };
                    match prepared {
                        Ok(terminal) => {
                            state.terminals.insert(id, terminal);
                            // Reserve the spawn key before calling the supervisor.
                            self.commit_agent(saved, state, changes, None, json!({}))?;
                            continue;
                        }
                        Err(error) => {
                            effects::finish(&request, Err(error), &mut changes);
                            self.commit_agent(saved, state, changes, None, json!({}))?;
                            continue;
                        }
                    }
                }
                let terminal = state.terminals.get_mut(&id).expect("reserved terminal");
                let instance = match self.pty_request(Request::Spawn(terminal.spawn.clone())) {
                    Ok(Response::Instance(instance)) => instance,
                    Err(error) if error.data.kind == ErrorKind::HarnessUnavailable => {
                        // This explicit supervisor result proves that no process
                        // survived. Ambiguous transport/storage failures still
                        // retain the spawn key for reconciliation.
                        state.terminals.remove(&id);
                        effects::finish(
                            &request,
                            Err(effects::failure(error.message)),
                            &mut changes,
                        );
                        self.commit_agent(saved, state, changes, None, json!({}))?;
                        continue;
                    }
                    Err(error) => return Err(error),
                    Ok(_) => {
                        return Err(errors::error(
                            ErrorKind::PtydUnavailable,
                            "Invalid terminal spawn response",
                        ));
                    }
                };
                terminal.instance_id = Some(instance.instance_id);
                terminal.supervisor_id = Some(instance.supervisor_id);
                Ok(json!({"terminalId":id}))
            } else if let Some(id) = request.params["terminalId"]
                .as_str()
                .filter(|id| state.terminals.contains_key(*id))
            {
                let terminal = &state.terminals[id];
                match request.method.as_str() {
                    "terminal/output" => Ok(terminal.output()),
                    "terminal/wait_for_exit" => {
                        if terminal.exit_code.is_none() {
                            continue;
                        }
                        Ok(terminal.status())
                    }
                    "terminal/kill" | "terminal/release" => {
                        let instance_id = terminal.instance_id.clone().ok_or_else(|| {
                            errors::error(ErrorKind::PaneBusy, "Terminal is still starting")
                        })?;
                        self.pty_request(if request.method == "terminal/kill" {
                            Request::Kill { instance_id }
                        } else {
                            Request::Close { instance_id }
                        })?;
                        if request.method == "terminal/release" {
                            state.terminals.remove(id);
                        }
                        Ok(json!({}))
                    }
                    _ => Err(effects::failure("Unknown terminal method")),
                }
            } else {
                Err(effects::failure("Terminal does not belong to this session"))
            };
            effects::finish(&request, result, &mut changes);
            self.commit_agent(saved, state, changes, None, json!({}))?;
        }
        Ok(())
    }
}
