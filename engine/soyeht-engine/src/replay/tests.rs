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
                    // The writer does not send what a subscription that ended left queued.
                    if packet.is_current() {
                        frames.push(packet.frame);
                    }
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

    /// A client that subscribed the one terminal from the start, with the first slice's read
    /// from the disk on its way.
    fn with_a_disk_read_in_flight() -> (Self, String) {
        let mut rig = Self::new(1);
        let pane = rig.panes[0].clone();
        rig.subscribe_from_start(&pane);
        rig.tick();
        (rig, pane)
    }

    /// Nothing more comes once the replays have ended; then the rig is removed.
    async fn end(mut self) {
        self.tick_for(Duration::from_millis(100));
        assert!(self.read(usize::MAX).await.is_empty());
        remove_after(self.core, self.root);
    }

    /// Ticks and reads what the engine queues until `handle`, which gets each frame, says the
    /// replays have ended. The queue never holds more than the replay's share.
    async fn read_until(&mut self, mut handle: impl FnMut(Frame) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            assert!(Instant::now() < deadline, "the replays never ended");
            self.tick();
            for frame in self.read(usize::MAX).await {
                if handle(frame) {
                    return;
                }
            }
            assert!(self.queued() <= QUEUE_BYTES + 1024);
        }
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
    rig.read_until(|frame| {
        if let Frame::Terminal { offset, bytes, .. } = &frame {
            assert_eq!(*offset, next as u64);
            assert!(
                bytes
                    .iter()
                    .enumerate()
                    .all(|(i, b)| *b == pattern(next + i))
            );
            next += bytes.len();
        }
        is_done(&frame)
    })
    .await;
    assert_eq!(next, PANE_BYTES);
    rig.end().await;
}

#[tokio::test]
async fn the_first_subscribed_is_served_to_its_end_before_the_next() {
    let mut rig = Rig::new(3);
    for pane in rig.panes.clone() {
        rig.subscribe_from_start(&pane);
    }
    // Binary references are given in the order of the subscribes.
    let (mut finished, mut bytes) = (0, [0usize; 4]);
    rig.read_until(|frame| {
        if let Frame::Terminal {
            pane_ref, bytes: b, ..
        } = &frame
        {
            // Nothing of a replay is sent before the one ahead of it is done.
            assert_eq!(finished + 1, *pane_ref);
            bytes[*pane_ref as usize] += b.len();
        }
        finished += u32::from(is_done(&frame));
        finished == 3
    })
    .await;
    assert_eq!(bytes[1..], [PANE_BYTES; 3]);
    remove_after(rig.core, rig.root);
}

#[tokio::test]
async fn a_subscription_made_again_while_its_slice_is_read_is_replayed_once_whole() {
    let (mut rig, pane) = Rig::with_a_disk_read_in_flight();
    // The client subscribes the terminal again.
    rig.subscribe_from_start(&pane);
    let mut next = 0;
    rig.read_until(|frame| {
        if let Frame::Terminal {
            pane_ref,
            offset,
            bytes,
            ..
        } = &frame
        {
            assert_eq!(*pane_ref, 2, "bytes of the first subscription were sent");
            assert_eq!(*offset, next);
            next += bytes.len() as u64;
        }
        is_done(&frame)
    })
    .await;
    assert_eq!(next, PANE_BYTES as u64);
    rig.end().await;
}

#[tokio::test]
async fn a_connection_that_ends_while_its_slice_is_read_leaves_nothing_behind() {
    let (mut rig, _) = Rig::with_a_disk_read_in_flight();
    rig.core.disconnect(&rig.connection).unwrap();
    rig.tick_for(Duration::from_millis(300));
    assert!(rig.core.clients.is_empty());
    assert!(!rig.core.background.has_work());
    remove_after(rig.core, rig.root);
}

#[tokio::test]
async fn a_disk_that_cannot_be_read_ends_the_connection_instead_of_leaving_the_replay_unfinished() {
    let mut rig = Rig::new(1);
    let pane = rig.panes[0].clone();
    // What retention does, with nothing the screen could tell: the segments are gone.
    let segments = rig.root.join("sessions").join(&pane);
    for file in std::fs::read_dir(segments).unwrap() {
        std::fs::remove_file(file.unwrap().path()).unwrap();
    }
    rig.subscribe_from_start(&pane);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !rig.core.clients[&rig.connection].outbox.is_closed() {
        assert!(Instant::now() < deadline, "the connection was never ended");
        rig.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        rig.core.clients[&rig.connection].outbox.close_reason(),
        Some(crate::outbox::CloseReason::WorkerFailed)
    );
    remove_after(rig.core, rig.root);
}

#[tokio::test]
async fn a_subscription_that_is_dropped_while_its_slice_is_read_is_not_sent() {
    let (mut rig, pane) = Rig::with_a_disk_read_in_flight();
    // The client lets go of the terminal.
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
