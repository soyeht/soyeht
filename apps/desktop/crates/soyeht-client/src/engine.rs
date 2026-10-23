//! The engine handle the application holds: a supervisor thread that keeps one connection
//! alive, and a worker thread that carries out commands in the order they were issued.

use crate::{
    Error, Result,
    autostart::{self, Autostart},
    catalogue,
    commands::{self, Command, INPUT_CHUNK_BYTES},
    editor,
    event::Event,
    paging, preferences,
    reconnect::Backoff,
    rpc::Rpc,
    session::{EventSink, Session},
    transport, writing,
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
        Self::start_with(socket, Autostart::Off)
    }

    /// [`start`](Self::start), starting an engine when none answers and `autostart` says so.
    pub fn start_with(
        socket: PathBuf,
        autostart: Autostart,
    ) -> (Self, async_channel::Receiver<Event>) {
        let (events, inbox) = async_channel::bounded(EVENT_QUEUE);
        let (commands, queue) = mpsc::sync_channel(COMMAND_QUEUE);
        let (fetches, fetch_queue) = mpsc::sync_channel(COMMAND_QUEUE);
        let shared = Arc::new(Shared::default());
        {
            let (shared, events) = (Arc::clone(&shared), events.clone());
            std::thread::spawn(move || supervise(&socket, &autostart, &events, &shared));
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

fn supervise(socket: &Path, autostart: &Autostart, events: &EventSink, shared: &Shared) {
    let mut backoff = Backoff::default();
    // An engine is started once for each time none answers; a connection that was made allows
    // the next start, so an engine that ends later is started again but a start that fails is
    // not repeated at every retry.
    let mut may_start = true;
    while !shared.stopped.load(Ordering::Relaxed) {
        let ended = run_connection(socket, events, shared);
        if shared.rpc().is_some() {
            backoff.reset();
            may_start = true;
        }
        shared.set(None);
        if events
            .send_blocking(Event::Disconnected(ended.to_string()))
            .is_err()
        {
            return;
        }
        if let Autostart::On(plan) = autostart
            && may_start
            && engine_absent(&ended)
        {
            may_start = false;
            match start_engine(plan, socket, events, shared) {
                Started::Up => continue,
                Started::Failed => {}
                Started::Gone => return,
            }
        }
        wait(shared, backoff.next());
    }
}

/// Whether the connection failed because nothing listens at the endpoint.
fn engine_absent(error: &Error) -> bool {
    matches!(
        error,
        Error::Io(error) if matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
        )
    )
}

enum Started {
    /// An engine answers.
    Up,
    /// It could not be started; the person was told.
    Failed,
    /// The application is gone or stopping.
    Gone,
}

/// Starts an engine and says so. The start waits for the engine to answer, on this thread: until
/// then there is nothing to connect to.
fn start_engine(
    plan: &autostart::Plan,
    socket: &Path,
    events: &EventSink,
    shared: &Shared,
) -> Started {
    if events.send_blocking(Event::Starting).is_err() {
        return Started::Gone;
    }
    let stopped = || shared.stopped.load(Ordering::Relaxed);
    let host = autostart::Local {
        plan,
        socket,
        stopped: &stopped,
    };
    match autostart::ensure(plan, &host) {
        Ok(()) => {
            soyeht_platform::log::event("desktop.engine_ready", &[]);
            Started::Up
        }
        Err(autostart::StartError::Stopped) => Started::Gone,
        Err(error) => {
            soyeht_platform::log::failure("desktop.engine_start_failed", &error);
            let told = report(events, &error.to_string())
                && events
                    .send_blocking(Event::Disconnected(error.to_string()))
                    .is_ok();
            if told { Started::Failed } else { Started::Gone }
        }
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
                    .failure(&error)
                    .is_none_or(|event| events.send_blocking(event).is_ok());
                // A file the engine refused as text is told in its pane, not as a problem.
                released
                    && told
                    && (command.failure_is_shown_in_place(&error)
                        || report(events, &error.to_string()))
            }
        };
        if !delivered {
            return;
        }
    }
}

/// Tells the user. False when the application is gone and nobody can be told.
fn report(events: &EventSink, message: &str) -> bool {
    events
        .send_blocking(Event::Problem(message.to_owned()))
        .is_ok()
}

fn execute(rpc: &Rpc, command: &Command) -> Result<Option<Event>> {
    match command {
        Command::Input { pane_id, bytes } => return send_input(rpc, pane_id, bytes).map(|()| None),
        Command::Editor(editor) => return editor::commands::run(rpc, editor),
        Command::Writing(writing) => return writing::commands::run(rpc, writing),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autostart::Plan;

    fn absent_socket(name: &str) -> (PathBuf, PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("soyeht-engine-start-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        (directory.join("engine.sock"), directory)
    }

    fn next(inbox: &async_channel::Receiver<Event>) -> Event {
        inbox.recv_blocking().unwrap()
    }

    #[test]
    fn an_engine_that_cannot_be_found_is_started_once_and_the_failure_is_told() {
        let (socket, directory) = absent_socket("missing");
        let plan = Plan {
            directory: directory.clone(),
            programs: Some(directory.join("no-programs")),
        };
        let (engine, inbox) = Engine::start_with(socket, Autostart::On(plan));
        assert!(matches!(next(&inbox), Event::Disconnected(_)));
        assert!(matches!(next(&inbox), Event::Starting));
        let Event::Problem(message) = next(&inbox) else {
            panic!("expected the problem");
        };
        assert!(message.contains("soyeht-engine"), "{message}");
        assert!(matches!(next(&inbox), Event::Disconnected(_)));
        // The retry that follows does not start again: it only reports the engine missing.
        assert!(matches!(next(&inbox), Event::Disconnected(_)));
        std::thread::sleep(Duration::from_millis(300));
        engine.stop();
        // Nothing queued behind it starts an engine or tells of a failure.
        while let Ok(event) = inbox.try_recv() {
            assert!(
                matches!(event, Event::Disconnected(_)),
                "unexpected {event:?}"
            );
        }
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn without_a_start_the_engine_that_is_missing_is_only_reported() {
        let (socket, directory) = absent_socket("off");
        let (engine, inbox) = Engine::start_with(socket, Autostart::Off);
        assert!(matches!(next(&inbox), Event::Disconnected(_)));
        assert!(matches!(next(&inbox), Event::Disconnected(_)));
        engine.stop();
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
