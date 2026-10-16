//! The engine handle the application holds: a supervisor thread that keeps one connection
//! alive, and a worker thread that carries out commands in the order they were issued.

use crate::{
    Error, Result, catalogue,
    commands::{self, Command, INPUT_CHUNK_BYTES},
    editor,
    event::Event,
    paging, preferences,
    reconnect::Backoff,
    rpc::Rpc,
    session::{EventSink, Session},
    transport,
};
use soyeht_protocol::PaneInputResult;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// The `client` name sent in `hello`: `desktop` in a release build, `desktop-dev` in a debug one,
/// so the engine's log tells a person's window from a developer's.
pub const CLIENT_NAME: &str = if cfg!(debug_assertions) {
    "desktop-dev"
} else {
    "desktop"
};
/// How often a wait before reconnecting looks at whether the application has stopped.
const WAIT_SLICE: Duration = Duration::from_millis(100);
/// Engine events waiting for the interface. When it is full the reader waits, which stops it
/// taking bytes off the socket, so output the interface cannot draw fast enough slows the
/// engine down instead of growing memory.
const EVENT_QUEUE: usize = 1024;
/// Commands waiting for the worker. Keystrokes are merged, so reaching this means the engine
/// has stopped answering.
const COMMAND_QUEUE: usize = 1024;

#[derive(Default)]
struct Shared {
    rpc: Mutex<Option<Arc<Rpc>>>,
    stopped: AtomicBool,
}

impl Shared {
    fn rpc(&self) -> Option<Arc<Rpc>> {
        self.rpc
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self, rpc: Option<Arc<Rpc>>) {
        *self.rpc.lock().unwrap_or_else(PoisonError::into_inner) = rpc;
    }
}

pub struct Engine {
    shared: Arc<Shared>,
    commands: mpsc::SyncSender<Command>,
    /// Fetches wait in a queue of their own, served by a second worker.
    fetches: mpsc::SyncSender<Command>,
}

impl Engine {
    /// Starts connecting. Everything that happens arrives on the returned receiver.
    pub fn start(socket: PathBuf) -> (Self, async_channel::Receiver<Event>) {
        let (events, inbox) = async_channel::bounded(EVENT_QUEUE);
        let (commands, queue) = mpsc::sync_channel(COMMAND_QUEUE);
        let (fetches, fetch_queue) = mpsc::sync_channel(COMMAND_QUEUE);
        let shared = Arc::new(Shared::default());
        {
            let (shared, events) = (Arc::clone(&shared), events.clone());
            std::thread::spawn(move || supervise(&socket, &events, &shared));
        }
        // One worker for what the person does, one for fetching: a transcript per pane, usage and
        // the harness list queue up at every connection and must not delay a keystroke.
        for queue in [queue, fetch_queue] {
            let (shared, events) = (Arc::clone(&shared), events.clone());
            std::thread::spawn(move || work(&queue, &events, &shared));
        }
        (
            Self {
                shared,
                commands,
                fetches,
            },
            inbox,
        )
    }

    /// Queues a command. It runs after the ones before it. Never blocks the interface: when the
    /// queue is full the command is refused and the caller tells the user.
    pub fn send(&self, command: Command) -> Result<()> {
        let queue = if command.is_background() {
            &self.fetches
        } else {
            &self.commands
        };
        queue.try_send(command).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => Error::Busy,
            mpsc::TrySendError::Disconnected(_) => Error::Disconnected,
        })
    }

    /// Drops the current connection so the supervisor reconnects and starts again from a fresh
    /// snapshot, for when the replica can no longer be trusted.
    pub fn resync(&self) {
        if let Some(rpc) = self.shared.rpc() {
            rpc.close();
        }
    }

    /// Ends the connection and stops reconnecting.
    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::Relaxed);
        self.resync();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn supervise(socket: &Path, events: &EventSink, shared: &Shared) {
    let mut backoff = Backoff::default();
    while !shared.stopped.load(Ordering::Relaxed) {
        let ended = run_connection(socket, events, shared);
        if shared.rpc().is_some() {
            backoff.reset();
        }
        shared.set(None);
        if events
            .send_blocking(Event::Disconnected(ended.to_string()))
            .is_err()
        {
            return;
        }
        wait(shared, backoff.next());
    }
}

/// Waits for `delay`, or until the application stops the engine.
fn wait(shared: &Shared, delay: Duration) {
    let deadline = Instant::now() + delay;
    while !shared.stopped.load(Ordering::Relaxed) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(WAIT_SLICE));
    }
}

/// Connects, hands the application its state, and returns why the connection ended.
fn run_connection(socket: &Path, events: &EventSink, shared: &Shared) -> Error {
    let link = match transport::connect(socket) {
        Ok(link) => link,
        Err(error) => return error,
    };
    let session = match Session::open(link, events.clone()) {
        Ok(session) => session,
        Err(error) => return error,
    };
    let started = session.handshake(CLIENT_NAME).and_then(|snapshot| {
        shared.set(Some(Arc::clone(&session.rpc)));
        let since = snapshot.cursor;
        events
            .send_blocking(Event::Connected(snapshot))
            .map_err(|_| Error::Disconnected)?;
        session.subscribe_events(since)
    });
    match started {
        Ok(()) => session.wait(),
        Err(error) => {
            session.rpc.close();
            let reader = session.wait();
            // A handshake cut short by the connection ending is explained by how it ended.
            if matches!(error, Error::Disconnected) {
                reader
            } else {
                error
            }
        }
    }
}

fn work(queue: &mpsc::Receiver<Command>, events: &EventSink, shared: &Shared) {
    let mut carry = None;
    loop {
        let next = match carry.take() {
            Some(command) => command,
            None => match queue.recv() {
                Ok(command) => command,
                // Every sender is gone: the application is quitting.
                Err(_) => return,
            },
        };
        let command = commands::coalesce(next, queue, &mut carry);
        let outcome = match shared.rpc() {
            Some(rpc) => execute(&rpc, &command),
            None => Err(Error::Disconnected),
        };
        let delivered = match outcome {
            Ok(Some(event)) => events.send_blocking(event).is_ok(),
            Ok(None) => true,
            Err(error) => {
                // A request to open a pane that failed releases what waited for it.
                let released = command.ticket().is_none_or(|ticket| {
                    events.send_blocking(Event::OpenFailed { ticket }).is_ok()
                });
                // An editor command that failed leaves the replica waiting for its answer.
                let told = command
                    .failure()
                    .is_none_or(|event| events.send_blocking(event).is_ok());
                released && told && report(events, &error)
            }
        };
        if !delivered {
            return;
        }
    }
}

/// Tells the user. False when the application is gone and nobody can be told.
fn report(events: &EventSink, error: &Error) -> bool {
    events
        .send_blocking(Event::Problem(error.to_string()))
        .is_ok()
}

fn execute(rpc: &Rpc, command: &Command) -> Result<Option<Event>> {
    match command {
        Command::Input { pane_id, bytes } => return send_input(rpc, pane_id, bytes).map(|()| None),
        Command::Editor(editor) => return editor::commands::run(rpc, editor),
        Command::LoadQueue { pane_id } => return paging::queue(rpc, pane_id).map(Some),
        Command::LoadTasks { workspace_id } => return paging::tasks(rpc, workspace_id).map(Some),
        // A harness whose models cannot be read is told apart from a failed request: the
        // popover says so beside that harness and offers to try again.
        Command::LoadCatalogue { harness } => {
            return Ok(Some(match catalogue::load(rpc, harness) {
                Ok(catalogue) => Event::Catalogue {
                    harness: harness.clone(),
                    catalogue,
                },
                Err(error) => Event::CatalogueFailed {
                    harness: harness.clone(),
                    reason: error.to_string(),
                },
            }));
        }
        Command::LoadPreferences { harnesses } => {
            return preferences::load(rpc, harnesses)
                .map(|loaded| Some(Event::Preferences(loaded)));
        }
        _ => {}
    }
    match commands::request(command) {
        Some(request) => {
            let result = rpc.call_with_uploads(&request, &commands::uploads(command))?;
            commands::outcome(command, result)
        }
        None => Ok(None),
    }
}

fn send_input(rpc: &Rpc, pane_id: &str, bytes: &[u8]) -> Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        let end = bytes.len().min(offset + INPUT_CHUNK_BYTES);
        let result = rpc.call(&commands::input_request(pane_id, &bytes[offset..end]))?;
        let accepted = serde_json::from_value::<PaneInputResult>(result)?.accepted_bytes;
        let accepted = usize::try_from(accepted).unwrap_or(0);
        if accepted == 0 || accepted > end - offset {
            return Err(Error::Invalid(
                "the engine accepted an impossible number of bytes",
            ));
        }
        offset += accepted;
    }
    Ok(())
}
