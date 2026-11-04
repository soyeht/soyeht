//! One connection to the engine: the greeting, the handshake, and the reader thread that
//! routes everything the engine sends.

use crate::{
    Error, Result,
    device::Credential,
    event::Event,
    rpc::{Kind, Rpc},
    snapshot::Snapshot,
    streams::TerminalStreams,
    transport::Link,
    web_host::Host,
};
use serde_json::Value;
use soyeht_protocol::{
    AuthChallengeEvent, ClientCapability, EventName, EventsSubscribeParams, HelloParams,
    PaneClosedEvent, Request, RpcError, StateSnapshotParams, StateSnapshotResult, StreamInfo,
    WebCommandParams, Welcome, client_policy, frame::Frame, handshake, method,
    snapshot::SnapshotAssembler,
};
use std::{
    io::Read,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
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
    /// The engine's greeting, which a device signs in `hello`.
    challenge: AuthChallengeEvent,
    /// The welcome was accepted (for a device, its proof and role too). Until then the reader answers
    /// the requests and drops everything else the other end sends, so a server that has not proved
    /// itself cannot put state in the replica.
    accepted: Arc<AtomicBool>,
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
        let challenge = greeting?;

        let hosting = host.is_some();
        let rpc = Arc::new(Rpc::new(writer, close, 1));
        let assembly = Arc::new(Mutex::new(SnapshotAssembler::default()));
        let cursor = Arc::new(AtomicU64::new(0));
        let accepted = Arc::new(AtomicBool::new(false));
        let routing = Router {
            accepted: Arc::clone(&accepted),
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
            challenge,
            accepted,
        })
    }

    /// `hello`, then the full state. Afterwards [`Session::subscribe_events`] starts the feed.
    pub fn handshake(&self, client: &str) -> Result<Snapshot> {
        let mut hello = handshake::hello(client, env!("CARGO_PKG_VERSION"), None);
        if self.hosting {
            hello.capabilities = Some(vec![ClientCapability::WebHost]);
        }
        let welcome = self.hello(hello)?;
        handshake::expect_welcome(&welcome, false)?;
        self.accept(&welcome, client);
        self.snapshot()
    }

    /// As [`Session::handshake`] for a paired device: it proves itself with its key, and the engine
    /// has to prove itself with the key the pairing link named (ADR 0132).
    pub fn handshake_device(&self, client: &str, credential: &Credential) -> Result<Snapshot> {
        let (hello, expected) =
            credential.hello(client, env!("CARGO_PKG_VERSION"), &self.challenge)?;
        let welcome = self.hello(hello)?;
        expected.check(&welcome)?;
        handshake::expect_device_welcome(&welcome)?;
        self.accept(&welcome, client);
        self.snapshot()
    }

    /// The welcome passed every check: the engine may now push events.
    fn accept(&self, welcome: &Welcome, client: &str) {
        self.rpc.identify(welcome.role.clone(), client);
        self.accepted.store(true, Ordering::Release);
    }

    fn hello(&self, hello: HelloParams) -> Result<Welcome> {
        Ok(serde_json::from_value(
            self.rpc.call(&Request::Hello(hello))?,
        )?)
    }

    fn snapshot(&self) -> Result<Snapshot> {
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

fn read_greeting(reader: &mut impl Read) -> Result<AuthChallengeEvent> {
    let Some(Frame::Json(bytes)) = Frame::read_from(reader)? else {
        return Err(Error::Invalid("the engine did not greet"));
    };
    Ok(handshake::expect_challenge(&serde_json::from_slice(
        &bytes,
    )?)?)
}

struct Router {
    /// See [`Session::accepted`].
    accepted: Arc<AtomicBool>,
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
            // Output of a terminal belongs to a subscription, which only an accepted engine has.
            Frame::Terminal { .. } if !self.accepted.load(Ordering::Acquire) => Ok(()),
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
            // Before the welcome is accepted the other end is not known to be the engine: its
            // notifications and requests are ignored, only the answers to ours are read.
            if !self.accepted.load(Ordering::Acquire) {
                return Ok(());
            }
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
