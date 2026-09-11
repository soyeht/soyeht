use soyeht_protocol::{frame::Frame, limits};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
};
use tokio::sync::{Notify, mpsc, oneshot};

struct Budget {
    used: AtomicUsize,
    close_reason: AtomicU8,
    wake: Notify,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum CloseReason {
    ServerShutdown = 1,
    PeerClosed = 2,
    ReadError = 3,
    InvalidFrame = 4,
    InvalidJson = 5,
    HandshakeTimeout = 6,
    HandshakeRejected = 7,
    WorkerFailed = 8,
    SnapshotWriteFailed = 9,
    WriteError = 10,
    SlowConsumer = 11,
    DeviceRevoked = 12,
    StreamChanged = 13,
    WriterStopped = 14,
}

impl CloseReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServerShutdown => "server_shutdown",
            Self::PeerClosed => "peer_closed",
            Self::ReadError => "read_error",
            Self::InvalidFrame => "invalid_frame",
            Self::InvalidJson => "invalid_json",
            Self::HandshakeTimeout => "handshake_timeout",
            Self::HandshakeRejected => "handshake_rejected",
            Self::WorkerFailed => "worker_failed",
            Self::SnapshotWriteFailed => "snapshot_write_failed",
            Self::WriteError => "write_error",
            Self::SlowConsumer => "slow_consumer",
            Self::DeviceRevoked => "device_revoked",
            Self::StreamChanged => "stream_changed",
            Self::WriterStopped => "writer_stopped",
        }
    }

    fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::ServerShutdown,
            2 => Self::PeerClosed,
            3 => Self::ReadError,
            4 => Self::InvalidFrame,
            5 => Self::InvalidJson,
            6 => Self::HandshakeTimeout,
            7 => Self::HandshakeRejected,
            8 => Self::WorkerFailed,
            9 => Self::SnapshotWriteFailed,
            10 => Self::WriteError,
            11 => Self::SlowConsumer,
            12 => Self::DeviceRevoked,
            13 => Self::StreamChanged,
            14 => Self::WriterStopped,
            _ => return None,
        })
    }
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
    close_after_write: bool,
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
    pub fn closes_connection(&self) -> bool {
        self.close_after_write
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
        close_reason: AtomicU8::new(0),
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
    pub fn close(&self, reason: CloseReason) {
        let _ = self.budget.close_reason.compare_exchange(
            0,
            reason as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.budget.wake.notify_one();
    }
    pub fn is_closed(&self) -> bool {
        self.close_reason().is_some()
    }
    pub fn close_reason(&self) -> Option<CloseReason> {
        CloseReason::from_byte(self.budget.close_reason.load(Ordering::Acquire))
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
            self.close(CloseReason::SlowConsumer);
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
            close_after_write: false,
        };
        let sender = if control {
            &self.control
        } else {
            &self.terminal
        };
        if sender.send(packet).is_err() {
            self.close(CloseReason::WriterStopped);
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
                close_after_write: false,
            })
            .map_err(|_| "connection closed")?;
        written.await.map_err(|_| "connection closed")
    }
}

impl Receiver {
    pub fn close_reason(&self) -> Option<CloseReason> {
        CloseReason::from_byte(self.budget.close_reason.load(Ordering::Acquire))
    }

    pub async fn closed(&self) -> CloseReason {
        loop {
            let notified = self.budget.wake.notified();
            if let Some(reason) = self.close_reason() {
                return reason;
            }
            notified.await;
        }
    }

    fn closing_packet(&mut self) -> crate::errors::Result<Option<Packet>> {
        // Free queued bytes and wake blocked snapshot writers before the final error.
        self.control.close();
        self.terminal.close();
        while self.control.try_recv().is_ok() {}
        while self.terminal.try_recv().is_ok() {}
        if self.reported_close || self.close_reason() != Some(CloseReason::SlowConsumer) {
            return Ok(None);
        }
        self.reported_close = true;
        Ok(Some(Packet {
            frame: crate::model::failure(
                serde_json::Value::Null,
                crate::errors::error(
                    soyeht_protocol::ErrorKind::SlowConsumer,
                    "Output queue exceeded; request a new snapshot",
                ),
            )?,
            _permit: None,
            sent: None,
            current: None,
            close_after_write: true,
        }))
    }

    pub async fn next(&mut self) -> crate::errors::Result<Option<Packet>> {
        if self.close_reason().is_some() {
            return self.closing_packet();
        }
        tokio::select! {
            biased;
            _ = self.budget.wake.notified() => self.closing_packet(),
            Some(packet) = self.control.recv() => Ok(Some(packet)),
            Some(packet) = self.terminal.recv() => Ok(Some(packet)),
            else => Ok(None),
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
        assert!(!receiver.next().await.unwrap().unwrap().is_current());
        assert!(!receiver.next().await.unwrap().unwrap().is_current());
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
            receiver.next().await.unwrap().unwrap().frame,
            Frame::Json(_)
        ));
        while outbox.send(Frame::Terminal {
            pane_ref: 1,
            generation: 1,
            offset: 0,
            bytes: vec![1; 65536],
        }) {}
        assert!(outbox.is_closed());
        assert_eq!(outbox.close_reason(), Some(CloseReason::SlowConsumer));
        assert!(outbox.budget.used.load(Ordering::Acquire) <= limits::CONNECTION_QUEUE_BYTES);
        let Frame::Json(bytes) = receiver.next().await.unwrap().unwrap().frame else {
            panic!("missing close reason")
        };
        let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error["id"], serde_json::Value::Null);
        assert_eq!(error["error"]["data"]["kind"], "slow_consumer");
        assert_eq!(
            error["error"]["code"],
            soyeht_protocol::error_code(&soyeht_protocol::ErrorKind::SlowConsumer)
        );
        assert!(receiver.next().await.unwrap().is_none());
        drop(receiver);
        assert_eq!(outbox.budget.used.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn first_close_reason_wins_and_wakes_the_writer() {
        let (outbox, receiver) = channel();
        outbox.close(CloseReason::StreamChanged);
        outbox.close(CloseReason::ServerShutdown);
        assert_eq!(receiver.closed().await, CloseReason::StreamChanged);
        assert_eq!(outbox.close_reason(), Some(CloseReason::StreamChanged));
    }

    #[tokio::test]
    async fn snapshot_waits_for_transport_completion_without_spending_live_budget() {
        let (outbox, mut receiver) = channel();
        let writing = outbox.clone();
        let task =
            tokio::spawn(async move { writing.write_page(Frame::Json(b"{}".to_vec())).await });
        let mut packet = receiver.next().await.unwrap().unwrap();
        assert!(!task.is_finished());
        assert_eq!(outbox.budget.used.load(Ordering::Acquire), 0);
        packet.written();
        assert!(task.await.unwrap().is_ok());
    }
}
