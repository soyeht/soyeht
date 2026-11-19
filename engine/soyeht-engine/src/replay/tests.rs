use super::*;
use crate::{core::Core, outbox, test_panes::insert_terminal_pane, test_paths::remove_after};
use serde_json::Value;
use soyeht_protocol::{Actor, Role, event, frame::Frame};
use std::time::{Duration, Instant};

const PANE_BYTES: usize = 3 * 1024 * 1024;

/// A core with `panes` terminals that each hold `PANE_BYTES` of output, written to the disk as
/// well as kept in memory, and a client of it that reads nothing yet.
struct Rig {
    core: Core,
    root: std::path::PathBuf,
    panes: Vec<String>,
    connection: String,
    queue: outbox::Receiver,
}

fn pattern(offset: usize) -> u8 {
    (offset % 251) as u8
}

impl Rig {
    fn new(panes: usize) -> Self {
        let root = std::env::temp_dir().join(format!("soyeht-replay-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut core = Core::open(&root).unwrap();
        let panes: Vec<String> = (0..panes)
            .map(|n| {
                let id = insert_terminal_pane(&mut core, &root, &format!("shell{n}"), false);
                let screen = &mut core.panes.get_mut(&id).unwrap().screen;
                screen.recording = true;
                for chunk in 0..PANE_BYTES / limits::TERMINAL_PAYLOAD_BYTES {
                    let start = chunk * limits::TERMINAL_PAYLOAD_BYTES;
                    let bytes = (start..start + limits::TERMINAL_PAYLOAD_BYTES)
                        .map(pattern)
                        .collect();
                    screen.append_data(start as u64, bytes).unwrap();
                }
                id
            })
            .collect();
        let (outbox, queue) = outbox::channel();
        let connection = core.connect(true, false, outbox).unwrap();
        core.clients.get_mut(&connection).unwrap().actor = Some(Actor {
            role: Role::HumanDevice,
            client: "test".into(),
            device_id: None,
            pane_id: None,
        });
        Self {
            core,
            root,
            panes,
            connection,
            queue,
        }
    }

    /// Subscribes `pane` from the start of what its screen holds: the part the memory of the
    /// screen no longer has comes from the disk.
    fn subscribe_from_start(&mut self, pane: &str) {
        let stream_id = self.core.panes[pane].stored.pane.stream_id.clone().unwrap();
        self.core
            .stream_request(
                &self.connection,
                json!(1),
                soyeht_protocol::method::PANE_STREAM_SUBSCRIBE,
                &pane_params(pane, json!({"streamId":stream_id,"offset":"0"})),
            )
            .unwrap();
    }

    /// What the engine has queued, at most `bytes` of it, as the writer would take it.
    async fn read(&mut self, bytes: usize) -> Vec<Frame> {
        let mut frames = Vec::new();
        let mut taken = 0;
        while taken < bytes {
            match tokio::time::timeout(Duration::from_millis(2), self.queue.next()).await {
                Ok(Ok(Some(packet))) => {
                    taken += match &packet.frame {
                        Frame::Terminal { bytes, .. } => bytes.len(),
                        _ => 0,
                    };
                    frames.push(packet.frame);
                }
                _ => break,
            }
        }
        frames
    }

    /// One tick: applies what the jobs finished and pumps.
    fn tick(&mut self) {
        self.core.apply_background();
        self.core.pump_streams().unwrap();
    }

    /// Ticks for `time`, as the engine does while the client is busy.
    fn tick_for(&mut self, time: Duration) {
        let deadline = Instant::now() + time;
        while Instant::now() < deadline {
            self.tick();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn queued(&self) -> usize {
        self.core.clients[&self.connection].outbox.queued()
    }
}

/// The parameters of a stream request for `pane`, from where `from` says unless it is null.
fn pane_params(pane: &str, from: Value) -> Value {
    let mut params = serde_json::Map::new();
    params.insert("paneId".into(), pane.into());
    if !from.is_null() {
        params.insert("from".into(), from);
    }
    params.into()
}

fn is_done(frame: &Frame) -> bool {
    match frame {
        Frame::Json(bytes) => {
            serde_json::from_slice::<Value>(bytes).unwrap()["method"] == event::STREAM_REPLAY_DONE
        }
        _ => false,
    }
}

#[tokio::test]
async fn a_replay_waits_in_slices_for_a_client_that_reads_nothing_and_arrives_whole_when_it_does() {
    let mut rig = Rig::new(1);
    let pane = rig.panes[0].clone();
    rig.subscribe_from_start(&pane);
    // The client reads nothing for a while: the engine queues a few slices and stops.
    rig.tick_for(Duration::from_millis(300));
    let waiting = rig.queued();
    assert!(waiting > 0 && waiting <= QUEUE_BYTES + 1024, "{waiting}");
    assert!(!rig.core.clients[&rig.connection].outbox.is_closed());

    // Then it reads, and every byte arrives in its place, the replay's end last.
    let mut next = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut done = false;
    while !done {
        assert!(Instant::now() < deadline, "the replay never ended");
        rig.tick();
        for frame in rig.read(usize::MAX).await {
            assert!(!done, "something followed the end of the replay");
            match &frame {
                Frame::Terminal { offset, bytes, .. } => {
                    assert_eq!(*offset, next as u64);
                    assert!(
                        bytes
                            .iter()
                            .enumerate()
                            .all(|(i, b)| *b == pattern(next + i))
                    );
                    next += bytes.len();
                }
                other => done = is_done(other),
            }
        }
        assert!(rig.queued() <= QUEUE_BYTES + 1024);
    }
    assert_eq!(next, PANE_BYTES);
    remove_after(rig.core, rig.root);
}

#[tokio::test]
async fn replays_take_turns_a_slice_each() {
    let mut rig = Rig::new(2);
    for pane in rig.panes.clone() {
        rig.subscribe_from_start(&pane);
    }
    let mut turns: Vec<u32> = Vec::new();
    let mut ended = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    while ended < 2 {
        assert!(Instant::now() < deadline, "the replays never ended");
        rig.tick();
        for frame in rig.read(usize::MAX).await {
            match frame {
                Frame::Terminal { pane_ref, .. } if turns.last() != Some(&pane_ref) => {
                    turns.push(pane_ref)
                }
                other if is_done(&other) => ended += 1,
                _ => {}
            }
        }
    }
    // Each turn is one slice (four frames of a record), so the panes alternate many times
    // over, rather than one replay going all the way before the other starts.
    assert!(turns.len() >= 2 * PANE_BYTES / SLICE_BYTES - 2, "{turns:?}");
    assert!(turns.windows(2).all(|pair| pair[0] != pair[1]));
    remove_after(rig.core, rig.root);
}

#[tokio::test]
async fn a_subscription_that_is_dropped_while_its_slice_is_read_is_not_sent() {
    let mut rig = Rig::new(1);
    let pane = rig.panes[0].clone();
    rig.subscribe_from_start(&pane);
    rig.tick();
    // The disk read is on its way; the client lets go of the terminal.
    rig.core
        .stream_request(
            &rig.connection,
            json!(2),
            soyeht_protocol::method::PANE_STREAM_UNSUBSCRIBE,
            &pane_params(&pane, Value::Null),
        )
        .unwrap();
    rig.tick_for(Duration::from_millis(300));
    let sent = rig.queued();
    for _ in 0..20 {
        rig.tick();
    }
    assert_eq!(rig.queued(), sent, "bytes were sent after the unsubscribe");
    assert!(!rig.core.clients[&rig.connection].replays.reading);
    remove_after(rig.core, rig.root);
}
