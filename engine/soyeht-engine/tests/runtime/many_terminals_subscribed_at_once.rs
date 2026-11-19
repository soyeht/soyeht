use super::*;
use soyeht_protocol::limits;

/// Enough terminals that their tails together pass the connection's queue.
const TERMINALS: usize = limits::CONNECTION_QUEUE_BYTES / limits::TAIL_BYTES + 2;
const LINE_BYTES: usize = 64;
/// How long the client reads nothing after it asked, as an app does while it parses a long replay.
const BUSY: Duration = Duration::from_secs(3);

/// A client that subscribes every terminal from the tail at once, as the Mac app did at launch,
/// and is busy for a while before it reads, is not disconnected, and every replay reaches it. The
/// engine queues a tail of up to `TAIL_BYTES` for each subscription, so the replays of several
/// terminals pass `CONNECTION_QUEUE_BYTES` and the connection is closed as a slow consumer. A
/// client that reads at once is not caught by this: the writer empties the queue as it fills.
#[test]
#[ignore = "fails until the engine replays in slices with backpressure; the Swift client paces its subscribes meanwhile"]
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
            let command = format!(
                "awk 'BEGIN{{for(i=1;i<={lines};i++)printf \"%06d {filler}\\n\",i}}'; touch done-{n}\n"
            );
            writer.call(
                "pane.input",
                json!({"paneId":pane,"bytes":STANDARD.encode(command),"idempotencyKey":format!("print-{n}")}),
            );
            pane
        })
        .collect();
    wait_until("every terminal has printed its output", || {
        (0..TERMINALS).all(|n| rig.path.join(format!("done-{n}")).exists())
    });

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
    let (mut replies, mut replays) = (0, 0);
    // The replays arrive one after the other, so the terminal frames between a `stream.replay_start`
    // and its `stream.replay_done` are that replay's bytes.
    let mut replay_bytes = 0;
    while replies < TERMINALS || replays < TERMINALS {
        let frame = match Frame::read_from(&mut reader.socket) {
            Ok(Some(frame)) => frame,
            other => panic!(
                "the connection ended after {replies} replies and {replays} replays of {TERMINALS}: {other:?}"
            ),
        };
        let bytes = match frame {
            Frame::Json(bytes) => bytes,
            Frame::Terminal { bytes, .. } => {
                replay_bytes += bytes.len();
                continue;
            }
            Frame::Attachment { .. } => continue,
        };
        let message: Value = serde_json::from_slice(&bytes).unwrap();
        assert_ne!(
            message["error"]["data"]["kind"], "slow_consumer",
            "the engine closed the connection after {replays} replays of {TERMINALS}"
        );
        if requests.iter().any(|id| message["id"] == *id) {
            assert!(
                message["error"].is_null(),
                "a subscribe was refused: {}",
                message["error"]
            );
            replies += 1;
        }
        match message["method"].as_str() {
            Some("stream.replay_start") => replay_bytes = 0,
            Some("stream.replay_done") => {
                assert!(
                    replay_bytes >= LINE_BYTES,
                    "replay {replays} of {TERMINALS} ended with {replay_bytes} bytes of output"
                );
                replays += 1;
            }
            _ => {}
        }
    }
    reader.call("engine.status", json!({}));
}
