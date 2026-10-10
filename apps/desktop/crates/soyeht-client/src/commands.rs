//! What the user can ask of the engine, and the typed protocol requests that carry it out.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_protocol::{
    PaneCloseParams, PaneCreateParams, PaneFocusParams, PaneInputParams, PaneMode,
    PaneResizeParams, PaneStreamSubscribeParams, ReplayFrom, Request, TailSelector,
};
use std::sync::mpsc;
use uuid::Uuid;

/// Raw input goes out in pieces of this size, as on the Mac client.
pub const INPUT_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
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
    },
    ClosePane {
        pane_id: String,
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
        Command::CreateTerminal { workspace_id } => Request::PaneCreate(PaneCreateParams {
            workspace_id: workspace_id.clone(),
            mode: PaneMode::Terminal,
            cwd: None,
            name: None,
            cols: None,
            rows: None,
            idempotency_key: key(),
            harness: None,
            model: None,
            effort: None,
            permission_mode: None,
            url: None,
            incognito: None,
            path: None,
            writing_kind: None,
            writing_section: None,
        }),
        Command::ClosePane { pane_id } => Request::PaneClose(PaneCloseParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
    })
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
