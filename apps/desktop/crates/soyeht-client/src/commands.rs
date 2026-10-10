//! What the user can ask of the engine, and the typed protocol requests that carry it out.

use crate::{Error, Result, agent::TRANSCRIPT_PAGE, event::Event};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use soyeht_protocol::{
    ConfigValue, HarnessInfo, HarnessListParams, HarnessListResult, PaneCancelParams,
    PaneCloseParams, PaneConfigSetParams, PaneCreateParams, PaneCreateResult, PaneFocusParams,
    PaneHarnessSwitchParams, PaneInputParams, PaneMode, PanePermissionAnswerParams,
    PanePromptParams, PaneResizeParams, PaneStreamSubscribeParams, PaneTranscriptParams,
    PaneTranscriptResult, PromptOrigin, ReplayFrom, Request, TailSelector, UsageListParams,
    UsageListResult, WorkspaceCloseParams, WorkspaceLayoutSetParams, WorkspacePaneLayoutEnvelope,
};
use std::sync::mpsc;
use uuid::Uuid;

/// Raw input goes out in pieces of this size, as on the Mac client.
pub const INPUT_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Input {
        pane_id: String,
        bytes: Vec<u8>,
    },
    Resize {
        pane_id: String,
        rows: u16,
        cols: u16,
    },
    Subscribe {
        pane_id: String,
    },
    Focus {
        pane_id: String,
    },
    CreateTerminal {
        workspace_id: String,
        /// Names this request in the answer (`Event::Opened`, `Event::OpenFailed`).
        ticket: u64,
    },
    ClosePane {
        pane_id: String,
    },
    /// A new agent pane running `harness`, started with the harness's own defaults.
    CreateAgent {
        workspace_id: String,
        harness: String,
        ticket: u64,
    },
    /// Send the person's message to an agent.
    Prompt {
        pane_id: String,
        text: String,
    },
    /// Stop the running turn.
    Cancel {
        pane_id: String,
    },
    LoadTranscript {
        pane_id: String,
    },
    LoadUsage,
    LoadHarnesses,
    /// Choose a value of one of the pane's agent options (model, effort, permission mode).
    SetConfig {
        pane_id: String,
        config_id: String,
        value: String,
    },
    SwitchHarness {
        pane_id: String,
        harness: String,
    },
    AnswerPermission {
        pane_id: String,
        request_id: String,
        option_id: String,
    },
    SetLayout {
        workspace_id: String,
        layout: WorkspacePaneLayoutEnvelope,
    },
    CloseWorkspace {
        workspace_id: String,
    },
}

fn key() -> String {
    Uuid::new_v4().to_string()
}

pub fn input_request(pane_id: &str, bytes: &[u8]) -> Request {
    Request::PaneInput(PaneInputParams {
        pane_id: pane_id.to_owned(),
        bytes: STANDARD.encode(bytes),
        idempotency_key: key(),
    })
}

/// The single request a command needs. Input is sent in pieces by the caller.
pub fn request(command: &Command) -> Option<Request> {
    Some(match command {
        Command::Input { .. } => return None,
        Command::Resize {
            pane_id,
            rows,
            cols,
        } => Request::PaneResize(PaneResizeParams {
            pane_id: pane_id.clone(),
            cols: u32::from(*cols),
            rows: u32::from(*rows),
            idempotency_key: key(),
        }),
        Command::Subscribe { pane_id } => Request::PaneStreamSubscribe(PaneStreamSubscribeParams {
            pane_id: pane_id.clone(),
            from: ReplayFrom::V0(TailSelector::Tail),
        }),
        Command::Focus { pane_id } => Request::PaneFocus(PaneFocusParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::CreateTerminal { workspace_id, .. } => {
            create(workspace_id, PaneMode::Terminal, None)
        }
        Command::CreateAgent {
            workspace_id,
            harness,
            ..
        } => create(workspace_id, PaneMode::Agent, Some(harness)),
        Command::ClosePane { pane_id } => Request::PaneClose(PaneCloseParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::Prompt { pane_id, text } => Request::PanePrompt(PanePromptParams {
            pane_id: pane_id.clone(),
            text: text.clone(),
            origin: Some(PromptOrigin::Human),
            attachments: None,
            idempotency_key: key(),
        }),
        Command::Cancel { pane_id } => Request::PaneCancel(PaneCancelParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::LoadTranscript { pane_id } => Request::PaneTranscript(PaneTranscriptParams {
            pane_id: pane_id.clone(),
            before: None,
            limit: Some(TRANSCRIPT_PAGE),
        }),
        Command::LoadUsage => Request::UsageList(UsageListParams {}),
        Command::LoadHarnesses => Request::HarnessList(HarnessListParams {
            cursor: None,
            limit: None,
            rescan: None,
        }),
        Command::SetConfig {
            pane_id,
            config_id,
            value,
        } => Request::PaneConfigSet(PaneConfigSetParams {
            pane_id: pane_id.clone(),
            config_id: config_id.clone(),
            value: ConfigValue::V0(value.clone()),
            idempotency_key: key(),
        }),
        Command::SwitchHarness { pane_id, harness } => {
            Request::PaneHarnessSwitch(PaneHarnessSwitchParams {
                pane_id: pane_id.clone(),
                harness: harness.clone(),
                model: None,
                effort: None,
                idempotency_key: key(),
            })
        }
        Command::AnswerPermission {
            pane_id,
            request_id,
            option_id,
        } => Request::PanePermissionAnswer(PanePermissionAnswerParams {
            pane_id: pane_id.clone(),
            request_id: request_id.clone(),
            option_id: option_id.clone(),
            idempotency_key: key(),
        }),
        Command::SetLayout {
            workspace_id,
            layout,
        } => Request::WorkspaceLayoutSet(WorkspaceLayoutSetParams {
            workspace_id: workspace_id.clone(),
            layout: layout.clone(),
            idempotency_key: key(),
        }),
        Command::CloseWorkspace { workspace_id } => Request::WorkspaceClose(WorkspaceCloseParams {
            workspace_id: workspace_id.clone(),
            idempotency_key: key(),
        }),
    })
}

fn create(workspace_id: &str, mode: PaneMode, harness: Option<&String>) -> Request {
    Request::PaneCreate(PaneCreateParams {
        workspace_id: workspace_id.to_owned(),
        mode,
        cwd: None,
        name: None,
        cols: None,
        rows: None,
        idempotency_key: key(),
        harness: harness.cloned(),
        model: None,
        effort: None,
        permission_mode: None,
        url: None,
        incognito: None,
        path: None,
        writing_kind: None,
        writing_section: None,
    })
}

impl Command {
    /// The request that opens a pane names it, so the answer can be matched to it.
    pub fn ticket(&self) -> Option<u64> {
        match self {
            Self::CreateTerminal { ticket, .. } | Self::CreateAgent { ticket, .. } => Some(*ticket),
            _ => None,
        }
    }

    /// Fetches that the interface does not wait on. They run apart from the commands that carry
    /// the person's typing, so a burst of them never delays a keystroke.
    pub fn is_background(&self) -> bool {
        matches!(
            self,
            Self::LoadTranscript { .. } | Self::LoadUsage | Self::LoadHarnesses
        )
    }
}

/// What the engine's answer to `command` tells the application, for the commands that fetch
/// something.
pub fn outcome(command: &Command, result: Value) -> Result<Option<Event>> {
    Ok(match command {
        Command::LoadTranscript { pane_id } => Some(Event::Transcript {
            pane_id: pane_id.clone(),
            items: serde_json::from_value::<PaneTranscriptResult>(result)?.items,
        }),
        Command::LoadUsage => Some(Event::Usage(
            serde_json::from_value::<UsageListResult>(result)?.providers,
        )),
        Command::LoadHarnesses => Some(Event::Harnesses(harnesses(result)?)),
        Command::CreateTerminal { ticket, .. } | Command::CreateAgent { ticket, .. } => {
            Some(Event::Opened {
                ticket: *ticket,
                pane_id: serde_json::from_value::<PaneCreateResult>(result)?.pane_id,
            })
        }
        _ => None,
    })
}

fn harnesses(result: Value) -> Result<Vec<HarnessInfo>> {
    serde_json::from_value::<HarnessListResult>(result)
        .map(|list| list.items)
        .map_err(Error::from)
}

/// Merges queued commands that a single request can carry: keystrokes for the same pane go out
/// in one round trip, and of several resizes of one pane only the last matters. A command of
/// another kind stays in order: it is returned in `carry`.
pub fn coalesce(
    first: Command,
    queue: &mpsc::Receiver<Command>,
    carry: &mut Option<Command>,
) -> Command {
    match first {
        Command::Input { pane_id, bytes } => merge_input(pane_id, bytes, queue, carry),
        Command::Resize {
            pane_id,
            rows,
            cols,
        } => merge_resize(pane_id, (rows, cols), queue, carry),
        other => other,
    }
}

fn merge_input(
    pane_id: String,
    mut bytes: Vec<u8>,
    queue: &mpsc::Receiver<Command>,
    carry: &mut Option<Command>,
) -> Command {
    while bytes.len() < INPUT_CHUNK_BYTES {
        match queue.try_recv() {
            Ok(Command::Input {
                pane_id: next_pane,
                bytes: more,
            }) if next_pane == pane_id => {
                bytes.extend_from_slice(&more);
            }
            Ok(other) => {
                *carry = Some(other);
                break;
            }
            Err(_) => break,
        }
    }
    Command::Input { pane_id, bytes }
}

fn merge_resize(
    pane_id: String,
    mut size: (u16, u16),
    queue: &mpsc::Receiver<Command>,
    carry: &mut Option<Command>,
) -> Command {
    loop {
        match queue.try_recv() {
            Ok(Command::Resize {
                pane_id: next_pane,
                rows,
                cols,
            }) if next_pane == pane_id => {
                size = (rows, cols);
            }
            Ok(other) => {
                *carry = Some(other);
                break;
            }
            Err(_) => break,
        }
    }
    Command::Resize {
        pane_id,
        rows: size.0,
        cols: size.1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn input(pane: &str, text: &str) -> Command {
        Command::Input {
            pane_id: pane.into(),
            bytes: text.as_bytes().to_vec(),
        }
    }

    #[test]
    fn typing_bursts_for_one_pane_merge_in_order() {
        let (tx, rx) = mpsc::channel();
        tx.send(input("p", "b")).unwrap();
        tx.send(input("p", "c")).unwrap();
        let mut carry = None;
        assert_eq!(
            coalesce(input("p", "a"), &rx, &mut carry),
            input("p", "abc")
        );
        assert_eq!(carry, None);
    }

    #[test]
    fn merging_stops_at_another_pane_or_command_and_keeps_order() {
        let (tx, rx) = mpsc::channel();
        tx.send(input("p", "b")).unwrap();
        tx.send(input("q", "x")).unwrap();
        tx.send(input("p", "c")).unwrap();
        let mut carry = None;
        assert_eq!(coalesce(input("p", "a"), &rx, &mut carry), input("p", "ab"));
        assert_eq!(carry, Some(input("q", "x")));
        assert_eq!(rx.try_recv().unwrap(), input("p", "c"));
    }

    #[test]
    fn a_burst_of_resizes_keeps_only_the_last_size() {
        let resize = |rows, cols| Command::Resize {
            pane_id: "p".into(),
            rows,
            cols,
        };
        let (tx, rx) = mpsc::channel();
        tx.send(resize(25, 81)).unwrap();
        tx.send(resize(30, 100)).unwrap();
        tx.send(Command::Focus {
            pane_id: "p".into(),
        })
        .unwrap();
        let mut carry = None;
        assert_eq!(coalesce(resize(24, 80), &rx, &mut carry), resize(30, 100));
        assert_eq!(
            carry,
            Some(Command::Focus {
                pane_id: "p".into()
            })
        );
    }

    #[test]
    fn other_commands_pass_through_untouched() {
        let (_tx, rx) = mpsc::channel();
        let focus = Command::Focus {
            pane_id: "p".into(),
        };
        assert_eq!(coalesce(focus.clone(), &rx, &mut None), focus);
    }

    #[test]
    fn requests_use_the_generated_method_names_and_wire_shapes() {
        let body = |command: Command| -> Value {
            serde_json::to_value(request(&command).unwrap()).unwrap()
        };
        let subscribe = body(Command::Subscribe {
            pane_id: "p".into(),
        });
        assert_eq!(subscribe["method"], "pane.stream.subscribe");
        assert_eq!(
            subscribe["params"],
            serde_json::json!({"paneId": "p", "from": "tail"})
        );
        let resize = body(Command::Resize {
            pane_id: "p".into(),
            rows: 24,
            cols: 80,
        });
        assert_eq!(resize["method"], "pane.resize");
        assert_eq!(
            (
                resize["params"]["rows"].clone(),
                resize["params"]["cols"].clone()
            ),
            (24.into(), 80.into())
        );
        let create = body(Command::CreateTerminal {
            workspace_id: "w".into(),
            ticket: 7,
        });
        assert_eq!(create["method"], "pane.create");
        assert_eq!(create["params"]["mode"], "terminal");
        assert!(
            create["params"]["idempotencyKey"]
                .as_str()
                .is_some_and(|key| key.len() == 36)
        );
    }

    #[test]
    fn only_fetches_run_in_the_background_and_only_opening_a_pane_has_a_ticket() {
        assert!(Command::LoadUsage.is_background());
        assert!(
            Command::LoadTranscript {
                pane_id: "p".into()
            }
            .is_background()
        );
        assert!(!input("p", "x").is_background());
        let open = Command::CreateTerminal {
            workspace_id: "w".into(),
            ticket: 3,
        };
        assert_eq!(open.ticket(), Some(3));
        assert_eq!(input("p", "x").ticket(), None);
    }

    #[test]
    fn the_answer_to_opening_a_pane_carries_its_ticket() {
        let open = Command::CreateTerminal {
            workspace_id: "w".into(),
            ticket: 3,
        };
        let event = outcome(&open, serde_json::json!({"paneId": "p9"})).unwrap();
        assert!(matches!(event, Some(Event::Opened { ticket: 3, pane_id }) if pane_id == "p9"));
    }

    #[test]
    fn input_is_base64_with_a_fresh_idempotency_key_each_time() {
        let first = serde_json::to_value(input_request("p", b"ls\r")).unwrap();
        let second = serde_json::to_value(input_request("p", b"ls\r")).unwrap();
        assert_eq!(first["method"], "pane.input");
        assert_eq!(first["params"]["bytes"], "bHMN");
        assert_ne!(
            first["params"]["idempotencyKey"],
            second["params"]["idempotencyKey"]
        );
    }
}
