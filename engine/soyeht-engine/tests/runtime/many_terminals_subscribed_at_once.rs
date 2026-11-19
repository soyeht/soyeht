use super::*;
use soyeht_protocol::limits;
use std::collections::HashMap;

/// About the most terminals a person has open at once (ADR 0168): their tails together are
/// sixteen times the connection's queue.
const TERMINALS: usize = 64;
const LINE_BYTES: usize = 64;
/// How long the client reads nothing after it asked, as an app does while it parses a long replay.
const BUSY: Duration = Duration::from_secs(3);

/// A client that subscribes every terminal from the tail at once, as the Mac app did at launch,
/// and is busy for a while before it reads, is not disconnected, and every replay reaches it whole.
/// The engine sends a replay in slices, only as the connection's queue drains (ADR 0168): a queue
/// that took the tail of every subscription at once would pass `CONNECTION_QUEUE_BYTES` and the
/// connection would be closed as a slow consumer. A client that reads at once is not caught by
/// this: the writer empties the queue as it fills.
#[test]
fn subscribing_every_terminal_without_waiting_does_not_disconnect_the_client() {
    let rig = Rig::new();
    let mut writer = rig.client();
    let workspace = writer.add_workspace(&rig.path, "many-terminals");
    let lines = limits::TAIL_BYTES * 5 / 4 / LINE_BYTES;
    let filler = ".".repeat(LINE_BYTES - 7);
    let panes: Vec<String> = (0..TERMINALS)
        .map(|n| {
            let pane = writer.call(
                "pane.create",
                json!({"workspaceId":workspace,"mode":"terminal","idempotencyKey":format!("pane-{n}")}),
            )["paneId"]
                .as_str()
                .unwrap()
                .to_owned();
            wait_until("the shell starts", || !writer.pane(&pane)["pid"].is_null());
            // The program prints more than a tail, then leaves a file: no sleeping on its output.
            // One terminal prints at a time, so the supervisor keeps every byte of it.
            let command = format!(
                "awk 'BEGIN{{for(i=1;i<={lines};i++)printf \"%06d {filler}\\n\",i}}'; touch done-{n}\n"
            );
            writer.call(
                "pane.input",
                json!({"paneId":pane,"bytes":STANDARD.encode(command),"idempotencyKey":format!("print-{n}")}),
            );
            wait_until("the terminal has printed its output", || {
                rig.path.join(format!("done-{n}")).exists()
            });
            pane
        })
        .collect();

    // The shell is done when its file is, but the engine may not have taken in all its output.
    let mut probe = rig.client();
    for pane in &panes {
        wait_until("the engine has the whole output of the terminal", || {
            let head = probe.call(
                "pane.stream.subscribe",
                json!({"paneId":pane,"from":"tail"}),
            )["nextOffset"]
                .as_str()
                .unwrap()
                .parse::<usize>()
                .unwrap();
            probe.call("pane.stream.unsubscribe", json!({"paneId":pane}));
            head >= lines * (LINE_BYTES + 1)
        });
    }
    let mut reader = rig.client();
    let requests: Vec<u64> = panes
        .iter()
        .map(|pane| {
            reader.send(
                "pane.stream.subscribe",
                json!({"paneId":pane,"from":"tail"}),
            )
        })
        .collect();
    // The engine's log says when it gives up on the connection; the client has read nothing.
    let busy_until = Instant::now() + BUSY;
    while Instant::now() < busy_until {
        let log = fs::read_to_string(rig.path.join("engine.log")).unwrap_or_default();
        assert!(
            !log.contains("slow_consumer"),
            "the engine closed the connection of a client that had not read yet"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let mut replays = Replays::default();
    while replays.replies < TERMINALS || replays.done < TERMINALS {
        let frame = match Frame::read_from(&mut reader.socket) {
            Ok(Some(frame)) => frame,
            other => panic!(
                "the connection ended after {} replies and {} replays of {TERMINALS}: {other:?}",
                replays.replies, replays.done
            ),
        };
        match frame {
            Frame::Json(bytes) => {
                replays.message(&requests, serde_json::from_slice(&bytes).unwrap())
            }
            Frame::Terminal {
                pane_ref,
                offset,
                bytes,
                ..
            } => replays.bytes(pane_ref, offset, bytes.len()),
            Frame::Attachment { .. } => {}
        }
    }
    reader.call("engine.status", json!({}));
}

/// What the client has read of the replays, by stream.
#[derive(Default)]
struct Replays {
    replies: usize,
    done: usize,
    /// The binary reference of each stream, from the reply to its subscribe.
    reference: HashMap<String, u64>,
    /// The offset each stream (by binary reference) had reached when it was subscribed.
    heads: HashMap<u64, u64>,
    /// The offset each replay (by binary reference) expects next, and what it must reach.
    expected: HashMap<u64, (u64, u64)>,
}

impl Replays {
    fn message(&mut self, requests: &[u64], message: Value) {
        assert_ne!(
            message["error"]["data"]["kind"], "slow_consumer",
            "the engine closed the connection after {} replays of {TERMINALS}",
            self.done
        );
        if requests.iter().any(|id| message["id"] == *id) {
            assert!(
                message["error"].is_null(),
                "a subscribe was refused: {}",
                message["error"]
            );
            let result = &message["result"];
            self.reference.insert(
                result["streamId"].as_str().unwrap().to_owned(),
                result["binaryPaneRef"].as_u64().unwrap(),
            );
            self.heads.insert(
                result["binaryPaneRef"].as_u64().unwrap(),
                result["nextOffset"].as_str().unwrap().parse().unwrap(),
            );
            self.replies += 1;
        }
        let params = &message["params"];
        let offset = |name: &str| params[name].as_str().unwrap().parse::<u64>().unwrap();
        match message["method"].as_str() {
            Some("stream.replay_start") => {
                let reference = self.reference[params["streamId"].as_str().unwrap()];
                let (from, until) = (offset("fromOffset"), offset("toOffset"));
                // The tail ends where the pane's output did when it was subscribed, and starts
                // 2 MiB before it, give or take the record boundary (contract 08, section 6).
                assert_eq!(until, self.heads[&reference]);
                assert!(
                    until - from
                        >= (limits::TAIL_BYTES - 2 * limits::TERMINAL_PAYLOAD_BYTES) as u64,
                    "a replay of {} bytes is not the tail",
                    until - from
                );
                self.expected.insert(reference, (from, until));
            }
            Some("stream.replay_done") => {
                let reference = self.reference[params["streamId"].as_str().unwrap()];
                let (reached, until) = self.expected[&reference];
                assert_eq!(
                    reached, until,
                    "replay {} ended short of its end",
                    self.done
                );
                assert_eq!(offset("nextOffset"), until);
                self.done += 1;
            }
            _ => {}
        }
    }

    /// A replay's bytes arrive in order and with no gap, from where its replay began.
    fn bytes(&mut self, reference: u32, offset: u64, length: usize) {
        let (next, _) = self
            .expected
            .get_mut(&u64::from(reference))
            .expect("terminal bytes before the replay began");
        assert_eq!(offset, *next, "a replay skipped or repeated output");
        *next += length as u64;
    }
}
