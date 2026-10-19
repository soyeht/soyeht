//! A printing shell loop in a pane and a record of when its frames reach a watcher.

use super::{Client, Rig};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use soyeht_protocol::frame::Frame;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// A shell loop that prints every 20 ms in a pane, and a recorder of when each frame reached
/// a watcher.
pub struct Ticker {
    stop: Arc<AtomicBool>,
    recorder: thread::JoinHandle<Vec<Instant>>,
}

impl Ticker {
    /// Starts the loop in `pane` and begins recording its live frames. The recorder reads
    /// blocking, so a read could not be split by a timeout.
    pub fn start(rig: &Rig, client: &mut Client, pane: &str) -> Self {
        let command = "while :; do printf 'tick\\n'; sleep 0.02; done\n";
        client.call(
            "pane.input",
            json!({"paneId":pane,"bytes":STANDARD.encode(command),"idempotencyKey":"ticker"}),
        );
        let mut watcher = rig.client();
        let (info, _) = watcher.tail(pane);
        let pane_ref = info["binaryPaneRef"].as_u64().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let recorder = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let mut arrivals = Vec::new();
                while !stop.load(Ordering::SeqCst) {
                    match Frame::read_from(&mut watcher.socket).unwrap() {
                        Some(Frame::Terminal {
                            pane_ref: found, ..
                        }) if u64::from(found) == pane_ref => arrivals.push(Instant::now()),
                        Some(_) => {}
                        None => panic!("engine closed the watcher"),
                    }
                }
                arrivals
            })
        };
        Self { stop, recorder }
    }

    /// Stops recording and returns when each frame arrived. The loop prints every 20 ms, so
    /// the recorder wakes soon after the stop.
    pub fn finish(self) -> Vec<Instant> {
        self.stop.store(true, Ordering::SeqCst);
        self.recorder.join().unwrap()
    }
}

/// The longest stretch inside `[from, to]` with no arrival.
pub fn longest_gap(arrivals: &[Instant], from: Instant, to: Instant) -> Duration {
    let mut previous = from;
    let mut longest = Duration::ZERO;
    for arrival in arrivals
        .iter()
        .filter(|arrival| **arrival > from && **arrival <= to)
    {
        longest = longest.max(*arrival - previous);
        previous = *arrival;
    }
    longest.max(to - previous)
}
