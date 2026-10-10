//! One connection to the engine: the greeting, the handshake, and the reader thread that
//! routes everything the engine sends.

use crate::{
    Error, Result,
    event::Event,
    rpc::{Kind, Rpc},
    snapshot::Snapshot,
    streams::TerminalStreams,
    transport::Link,
};
use serde_json::Value;
use soyeht_protocol::{
    EventName, EventsSubscribeParams, PaneClosedEvent, Request, RpcError, StateSnapshotParams,
    StateSnapshotResult, StreamInfo, Welcome, frame::Frame, handshake, snapshot::SnapshotAssembler,
};
use std::{
    io::Read,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};

/// The engine must greet within this time, as on the Mac client.
const GREETING_DEADLINE: Duration = Duration::from_secs(5);

pub type EventSink = async_channel::Sender<Event>;

pub struct Session {
    pub rpc: Arc<Rpc>,
    assembly: Arc<Mutex<SnapshotAssembler>>,
    cursor: Arc<AtomicU64>,
    reader: JoinHandle<Error>,
}

impl Session {
    /// Waits for the engine's greeting and starts routing its frames into `events`.
    pub fn open(link: Link, events: EventSink) -> Result<Self> {
        let Link {
            mut reader,
            writer,
            close,
        } = link;
        let (greeted, deadline) = mpsc::channel::<()>();
        let watchdog = Arc::clone(&close);
        std::thread::spawn(move || {
            if deadline.recv_timeout(GREETING_DEADLINE) == Err(mpsc::RecvTimeoutError::Timeout) {
                watchdog();
            }
        });
        let greeting = read_greeting(&mut reader);
        // Dropping the sender ends the watchdog, greeted or not.
        drop(greeted);
        greeting?;

        let rpc = Arc::new(Rpc::new(writer, close, 1));
        let assembly = Arc::new(Mutex::new(SnapshotAssembler::default()));
        let cursor = Arc::new(AtomicU64::new(0));
        let routing = Router {
            rpc: Arc::clone(&rpc),
            assembly: Arc::clone(&assembly),
            cursor: Arc::clone(&cursor),
            events,
            streams: TerminalStreams::default(),
        };
        let thread = std::thread::spawn(move || routing.run(reader));
        Ok(Self {
            rpc,
            assembly,
            cursor,
            reader: thread,
        })
    }

    /// `hello`, then the full state. Afterwards [`Session::subscribe_events`] starts the feed.
    pub fn handshake(&self, client: &str) -> Result<Snapshot> {
        let hello = Request::Hello(handshake::hello(client, env!("CARGO_PKG_VERSION"), None));
        let welcome: Welcome = serde_json::from_value(self.rpc.call(&hello)?)?;
        handshake::expect_welcome(&welcome, false)?;
        let result: StateSnapshotResult = serde_json::from_value(
            self.rpc
                .call(&Request::StateSnapshot(StateSnapshotParams {}))?,
        )?;
        let assembly =
            std::mem::take(&mut *self.assembly.lock().unwrap_or_else(PoisonError::into_inner));
        let snapshot = Snapshot::from_pages(assembly.finish(result)?)?;
        self.cursor.store(snapshot.cursor, Ordering::Relaxed);
        Ok(snapshot)
    }

    pub fn subscribe_events(&self, since: u64) -> Result<()> {
        let request = Request::EventsSubscribe(EventsSubscribeParams {
            since: since.to_string(),
        });
        self.rpc.call(&request).map(|_| ())
    }

    /// Blocks until the connection ends and says why.
    pub fn wait(self) -> Error {
        self.reader.join().unwrap_or(Error::Disconnected)
    }
}

fn read_greeting(reader: &mut impl Read) -> Result<()> {
    let Some(Frame::Json(bytes)) = Frame::read_from(reader)? else {
        return Err(Error::Invalid("the engine did not greet"));
    };
    handshake::expect_challenge(&serde_json::from_slice(&bytes)?)?;
    Ok(())
}

struct Router {
    rpc: Arc<Rpc>,
    assembly: Arc<Mutex<SnapshotAssembler>>,
    cursor: Arc<AtomicU64>,
    events: EventSink,
    streams: TerminalStreams,
}

impl Router {
    fn run(mut self, mut input: impl Read) -> Error {
        let error = loop {
            match Frame::read_from(&mut input) {
                Ok(Some(frame)) => {
                    if let Err(error) = self.route(frame) {
                        break error;
                    }
                }
                Ok(None) => break Error::Disconnected,
                Err(error) => break error.into(),
            }
        };
        self.rpc.fail_all();
        self.rpc.close();
        error
    }

    fn route(&mut self, frame: Frame) -> Result<()> {
        match frame {
            Frame::Json(bytes) => self.message(serde_json::from_slice(&bytes)?),
            Frame::Terminal {
                pane_ref,
                generation,
                offset,
                bytes,
            } => {
                let output = self.streams.accept(pane_ref, generation, offset, bytes)?;
                self.emit_terminal(output)
            }
            // Attachments belong to features this client does not have yet.
            Frame::Attachment { .. } => Ok(()),
        }
    }

    fn message(&mut self, message: Value) -> Result<()> {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            // An engine request would be a web host call; this client never claims that role.
            if message.get("id").is_none() {
                self.notification(
                    method,
                    message.get("params").cloned().unwrap_or(Value::Null),
                )?;
            }
            return Ok(());
        }
        match message.get("id").and_then(Value::as_u64) {
            Some(id) => self.response(id, &message),
            // An error without an id means the engine is closing the connection.
            None => match message.get("error") {
                Some(error) => Err(Error::Remote(serde_json::from_value::<RpcError>(
                    error.clone(),
                )?)),
                None => Ok(()),
            },
        }
    }

    fn response(&mut self, id: u64, message: &Value) -> Result<()> {
        // The caller may have timed out already.
        let Some(pending) = self.rpc.take(id) else {
            return Ok(());
        };
        if let Some(error) = message.get("error") {
            let error = serde_json::from_value::<RpcError>(error.clone())?;
            pending.complete(Err(Error::Remote(error)));
            return Ok(());
        }
        let result = message.get("result").cloned().unwrap_or(Value::Null);
        if pending.kind == Kind::StreamSubscribe {
            // Frames follow the answer immediately, so the stream is tracked before they do.
            let info = serde_json::from_value::<StreamInfo>(result.clone())?;
            self.streams.install(&info)?;
        }
        pending.complete(Ok(result));
        Ok(())
    }

    fn notification(&mut self, method: &str, params: Value) -> Result<()> {
        let Ok(name) = serde_json::from_value::<EventName>(Value::String(method.to_owned())) else {
            return Ok(());
        };
        if let Some(text) = params.get("eventId").and_then(Value::as_str) {
            let next: u64 = text
                .parse()
                .map_err(|_| Error::Invalid("event id is not a decimal"))?;
            if next <= self.cursor.load(Ordering::Relaxed) {
                return Ok(());
            }
            self.cursor.store(next, Ordering::Relaxed);
        }
        match name {
            EventName::StateSnapshotPart => {
                let part = serde_json::from_value(params)?;
                self.assembly
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(part)?;
                Ok(())
            }
            EventName::StreamReplayStart => {
                let output = self.streams.start(&serde_json::from_value(params)?)?;
                self.emit_terminal(output)
            }
            EventName::StreamGap => {
                let output = self.streams.gap(&serde_json::from_value(params)?)?;
                self.emit_terminal(output)
            }
            EventName::StreamReplayDone => self.streams.finish(&serde_json::from_value(params)?),
            EventName::PaneClosed => {
                // The pane's stream positions end with it, so they cannot pile up.
                let event: PaneClosedEvent = serde_json::from_value(params.clone())?;
                self.streams.remove(&event.pane_id);
                self.emit(Event::Notification { name, params })
            }
            _ => self.emit(Event::Notification { name, params }),
        }
    }

    fn emit_terminal(&self, output: Option<crate::streams::StreamOutput>) -> Result<()> {
        output.map_or(Ok(()), |output| self.emit(Event::Terminal(output)))
    }

    /// Blocks while the application has `EVENT_QUEUE` events waiting: a reader that waits stops
    /// taking bytes off the socket, so a flood of output slows the engine down instead of
    /// filling memory. A closed queue means the application is quitting.
    fn emit(&self, event: Event) -> Result<()> {
        self.events
            .send_blocking(event)
            .map_err(|_| Error::Disconnected)
    }
}
