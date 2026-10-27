//! One connection to the engine: the greeting, the handshake, and the reader thread that
//! routes everything the engine sends.

use crate::{
    Error, Result,
    event::Event,
    rpc::{Kind, Rpc},
    snapshot::Snapshot,
    streams::TerminalStreams,
    transport::Link,
    web_host::Host,
};
use serde_json::Value;
use soyeht_protocol::{
    ClientCapability, EventName, EventsSubscribeParams, PaneClosedEvent, Request, RpcError,
    StateSnapshotParams, StateSnapshotResult, StreamInfo, WebCommandParams, Welcome, client_policy,
    frame::Frame, handshake, method, snapshot::SnapshotAssembler,
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

/// The engine must greet within the contract's time, as on the Mac client.
const GREETING_DEADLINE: Duration = Duration::from_millis(client_policy::GREETING_TIMEOUT_MS);

pub type EventSink = async_channel::Sender<Event>;

pub struct Session {
    pub rpc: Arc<Rpc>,
    assembly: Arc<Mutex<SnapshotAssembler>>,
    cursor: Arc<AtomicU64>,
    reader: JoinHandle<Error>,
    /// This connection answers `web.command`, and says so in `hello`.
    hosting: bool,
}

impl Session {
    /// Waits for the engine's greeting and starts routing its frames into `events`. A `host`
    /// answers the engine's `web.command` requests; `handshake` then claims the web host role.
    pub fn open(link: Link, events: EventSink, host: Option<Arc<Host>>) -> Result<Self> {
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

        let hosting = host.is_some();
        let rpc = Arc::new(Rpc::new(writer, close, 1));
        let assembly = Arc::new(Mutex::new(SnapshotAssembler::default()));
        let cursor = Arc::new(AtomicU64::new(0));
        let routing = Router {
            rpc: Arc::clone(&rpc),
            assembly: Arc::clone(&assembly),
            cursor: Arc::clone(&cursor),
            events,
            streams: TerminalStreams::default(),
            host,
        };
        let thread = std::thread::spawn(move || routing.run(reader));
        Ok(Self {
            rpc,
            assembly,
            cursor,
            reader: thread,
            hosting,
        })
    }

    /// `hello`, then the full state. Afterwards [`Session::subscribe_events`] starts the feed.
    pub fn handshake(&self, client: &str) -> Result<Snapshot> {
        let mut hello = handshake::hello(client, env!("CARGO_PKG_VERSION"), None);
        if self.hosting {
            hello.capabilities = Some(vec![ClientCapability::WebHost]);
        }
        let hello = Request::Hello(hello);
        let welcome: Welcome = serde_json::from_value(self.rpc.call(&hello)?)?;
        handshake::expect_welcome(&welcome, false)?;
        self.rpc.identify(welcome.role.clone(), client);
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
    host: Option<Arc<Host>>,
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
        if let Some(name) = message.get("method").and_then(Value::as_str) {
            match message.get("id") {
                None => {
                    self.notification(name, message.get("params").cloned().unwrap_or(Value::Null))?
                }
                // The only request an engine makes of a client is a web host call.
                Some(id) => self.web_command(name, id, &message),
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

    /// Runs the engine's `web.command` on a thread of its own: the page may take a while, and
    /// the reader must go on taking what the engine sends. A connection that claimed no host
    /// role is never asked.
    fn web_command(&self, name: &str, id: &Value, message: &Value) {
        let (Some(host), true) = (&self.host, name == method::WEB_COMMAND) else {
            return;
        };
        let (host, rpc, id) = (Arc::clone(host), Arc::clone(&self.rpc), id.clone());
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let mut command = String::new();
            let outcome = match serde_json::from_value::<WebCommandParams>(params) {
                Ok(params) => {
                    command = serde_json::to_string(&params.command).unwrap_or_default();
                    host.execute(&params)
                }
                Err(error) => {
                    soyeht_platform::log::failure(
                        "desktop.web_command_unreadable",
                        &Error::from(error),
                    );
                    Host::refusal("The web command is invalid.")
                }
            };
            // What the host did and how long it took: a command slower than the engine's own wait
            // is otherwise a silence on both sides.
            let result = match &outcome.result {
                Ok(_) => "ok".to_owned(),
                Err(refusal) => format!("{:?}", refusal.data.kind),
            };
            soyeht_platform::log::event(
                "desktop.web_command",
                &[
                    ("command", command.trim_matches('"')),
                    ("result", &result),
                    ("ms", &started.elapsed().as_millis().to_string()),
                ],
            );
            if let Err(error) = rpc.respond(&id, outcome) {
                soyeht_platform::log::failure("desktop.web_host_response_failed", &error);
            }
        });
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
