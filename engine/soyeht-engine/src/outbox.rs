use soyeht_protocol::{frame::Frame, limits};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::{Notify, mpsc, oneshot};

struct Budget {
    used: AtomicUsize,
    closed: AtomicBool,
    slow: AtomicBool,
    wake: Notify,
}
struct Permit {
    budget: Arc<Budget>,
    size: usize,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.size, Ordering::AcqRel);
    }
}

pub struct Packet {
    pub frame: Frame,
    _permit: Option<Permit>,
    sent: Option<oneshot::Sender<()>>,
    current: Option<Arc<AtomicBool>>,
}
impl Packet {
    pub fn is_current(&self) -> bool {
        self.current
            .as_ref()
            .is_none_or(|flag| flag.load(Ordering::Acquire))
    }
    pub fn written(&mut self) {
        if let Some(sent) = self.sent.take() {
            let _ = sent.send(());
        }
    }
}

#[derive(Clone)]
pub struct Outbox {
    budget: Arc<Budget>,
    page_lock: Arc<tokio::sync::Mutex<()>>,
    control: mpsc::UnboundedSender<Packet>,
    terminal: mpsc::UnboundedSender<Packet>,
}
pub struct Receiver {
    reported_close: bool,
    budget: Arc<Budget>,
    control: mpsc::UnboundedReceiver<Packet>,
    terminal: mpsc::UnboundedReceiver<Packet>,
}

pub fn channel() -> (Outbox, Receiver) {
    let budget = Arc::new(Budget {
        used: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
        slow: AtomicBool::new(false),
        wake: Notify::new(),
    });
    let (control, control_rx) = mpsc::unbounded_channel();
    let (terminal, terminal_rx) = mpsc::unbounded_channel();
    (
        Outbox {
            budget: Arc::clone(&budget),
            page_lock: Arc::new(tokio::sync::Mutex::new(())),
            control,
            terminal,
        },
        Receiver {
            reported_close: false,
            budget,
            control: control_rx,
            terminal: terminal_rx,
        },
    )
}

impl Outbox {
    pub fn close(&self) {
        self.budget.closed.store(true, Ordering::Release);
        self.budget.wake.notify_one();
    }
    pub fn is_closed(&self) -> bool {
        self.budget.closed.load(Ordering::Acquire)
    }

    pub fn send(&self, frame: Frame) -> bool {
        let control = matches!(frame, Frame::Json(_));
        self.enqueue(frame, control)
    }

    /// Replay completion must follow its bytes in the terminal queue.
    pub fn send_ordered(&self, frame: Frame) -> bool {
        self.enqueue(frame, false)
    }

    pub fn send_stream(&self, frame: Frame, current: Arc<AtomicBool>) -> bool {
        self.enqueue_with_current(frame, false, Some(current))
    }

    fn enqueue(&self, frame: Frame, control: bool) -> bool {
        self.enqueue_with_current(frame, control, None)
    }

    fn enqueue_with_current(
        &self,
        frame: Frame,
        control: bool,
        current: Option<Arc<AtomicBool>>,
    ) -> bool {
        if self.is_closed() {
            return false;
        }
        let size = match &frame {
            Frame::Json(bytes) => bytes.len() + 5,
            Frame::Terminal { bytes, .. } | Frame::Attachment { bytes, .. } => bytes.len() + 21,
        };
        if self
            .budget
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(size)
                    .filter(|sum| *sum <= limits::CONNECTION_QUEUE_BYTES)
            })
            .is_err()
        {
            self.budget.slow.store(true, Ordering::Release);
            self.close();
            return false;
        }
        let packet = Packet {
            current,
            frame,
            _permit: Some(Permit {
                budget: Arc::clone(&self.budget),
                size,
            }),
            sent: None,
        };
        let sender = if control {
            &self.control
        } else {
            &self.terminal
        };
        if sender.send(packet).is_err() {
            self.close();
            return false;
        }
        true
    }

    /// Snapshot pages bypass the live queue budget, one page at a time. Return
    /// only after the writer confirms that the page reached the transport.
    pub async fn write_page(&self, frame: Frame) -> Result<(), &'static str> {
        let _serial = self.page_lock.lock().await;
        if self.is_closed() {
            return Err("connection closed");
        }
        let (sent, written) = oneshot::channel();
        self.control
            .send(Packet {
                current: None,
                frame,
                _permit: None,
                sent: Some(sent),
            })
            .map_err(|_| "connection closed")?;
        written.await.map_err(|_| "connection closed")
    }
}

impl Receiver {
    fn closing_packet(&mut self) -> Option<Packet> {
        // Free queued bytes and wake blocked snapshot writers before the final error.
        self.control.close();
        self.terminal.close();
        while self.control.try_recv().is_ok() {}
        while self.terminal.try_recv().is_ok() {}
        if self.reported_close || !self.budget.slow.load(Ordering::Acquire) {
            return None;
        }
        self.reported_close = true;
        Some(Packet {
            frame: crate::model::failure(
                serde_json::Value::Null,
                crate::errors::error(
                    soyeht_protocol::ErrorKind::SlowConsumer,
                    "Output queue exceeded; request a new snapshot",
                ),
            ),
            _permit: None,
            sent: None,
            current: None,
        })
    }

    pub async fn next(&mut self) -> Option<Packet> {
        if self.budget.closed.load(Ordering::Acquire) {
            return self.closing_packet();
        }
        tokio::select! {
            biased;
            _ = self.budget.wake.notified() => self.closing_packet(),
            Some(packet) = self.control.recv() => Some(packet),
            Some(packet) = self.terminal.recv() => Some(packet),
            else => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replaced_stream_invalidates_queued_bytes_and_completion() {
        let (outbox, mut receiver) = channel();
        let current = Arc::new(AtomicBool::new(true));
        outbox.send_stream(
            Frame::Terminal {
                pane_ref: 1,
                generation: 1,
                offset: 0,
                bytes: b"old".to_vec(),
            },
            Arc::clone(&current),
        );
        outbox.send_stream(
            Frame::Json(b"{\"method\":\"stream.replay_done\"}".to_vec()),
            Arc::clone(&current),
        );
        current.store(false, Ordering::Release);
        assert!(!receiver.next().await.unwrap().is_current());
        assert!(!receiver.next().await.unwrap().is_current());
        assert_eq!(outbox.budget.used.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn control_has_priority_and_queue_memory_is_bounded() {
        let (outbox, mut receiver) = channel();
        assert!(outbox.send(Frame::Terminal {
            pane_ref: 1,
            generation: 1,
            offset: 0,
            bytes: vec![1; 65536]
        }));
        assert!(outbox.send(Frame::Json(b"{}".to_vec())));
        assert!(matches!(
            receiver.next().await.unwrap().frame,
            Frame::Json(_)
        ));
        while outbox.send(Frame::Terminal {
            pane_ref: 1,
            generation: 1,
            offset: 0,
            bytes: vec![1; 65536],
        }) {}
        assert!(outbox.is_closed());
        assert!(outbox.budget.used.load(Ordering::Acquire) <= limits::CONNECTION_QUEUE_BYTES);
        let Frame::Json(bytes) = receiver.next().await.unwrap().frame else {
            panic!("missing close reason")
        };
        let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error["id"], serde_json::Value::Null);
        assert_eq!(error["error"]["data"]["kind"], "slow_consumer");
        assert_eq!(
            error["error"]["code"],
            soyeht_protocol::error_code(&soyeht_protocol::ErrorKind::SlowConsumer)
        );
        assert!(receiver.next().await.is_none());
        drop(receiver);
        assert_eq!(outbox.budget.used.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn snapshot_waits_for_transport_completion_without_spending_live_budget() {
        let (outbox, mut receiver) = channel();
        let writing = outbox.clone();
        let task =
            tokio::spawn(async move { writing.write_page(Frame::Json(b"{}".to_vec())).await });
        let mut packet = receiver.next().await.unwrap();
        assert!(!task.is_finished());
        assert_eq!(outbox.budget.used.load(Ordering::Acquire), 0);
        packet.written();
        assert!(task.await.unwrap().is_ok());
    }
}
