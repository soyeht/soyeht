//! The accepting side of an endpoint that asks for the proof: the connections a listener takes
//! are examined on threads of their own, and only those that proved they know the secret are
//! handed to whoever accepts. The loopback listener of Windows and the proved Unix socket
//! share it (ADR 0160).

use super::endpoint::{self, Admission, Proving, Token};
use crate::net::ConnectionErrors;
use std::{
    io::{self, Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::Duration,
};

/// The connections of a listener that proved the secret, and the thread that examines them.
pub(super) struct Accepting<S> {
    ready: Receiver<io::Result<S>>,
    stop: Arc<AtomicBool>,
    /// Makes the thread blocked in the listener's accept return, so it sees `stop`.
    wake: Box<dyn Fn() + Send + Sync>,
}

impl<S> Accepting<S>
where
    S: Proving + Send + Sync + 'static,
    for<'a> &'a S: Read + Write,
{
    /// Starts taking connections with `accept` (one call, one connection) and examining them
    /// against `token`, with room for `capacity` of them mid-exchange.
    pub(super) fn start(
        mut accept: impl FnMut() -> io::Result<S> + Send + 'static,
        token: Token,
        capacity: usize,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let (sender, ready) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        thread::Builder::new()
            .name("local-accept".into())
            .spawn(move || {
                serve(
                    std::iter::repeat_with(&mut accept),
                    ConnectionErrors::default(),
                    token,
                    &sender,
                    &stopping,
                    capacity,
                );
            })?;
        Ok(Self {
            ready,
            stop,
            wake: Box::new(wake),
        })
    }

    /// Waits for the next connection that proved it knows the secret. Fails with the error
    /// that stopped the listener, and with `BrokenPipe` on every call after that.
    pub(super) fn accept(&self) -> io::Result<S> {
        self.ready
            .recv()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "listener stopped"))?
    }
}

impl<S> Drop for Accepting<S> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the thread blocked in accept so it sees the flag and ends. When the accept
        // fails nothing is accepting any more, so the thread has already ended.
        (self.wake)();
    }
}

/// Takes the connections `incoming` yields (the accepts of one listener) until `stop`, or until
/// an error ends them: one that is not about a single connection, or a run of those that
/// `errors` gives up on.
pub(super) fn serve<S>(
    incoming: impl Iterator<Item = io::Result<S>>,
    mut errors: ConnectionErrors,
    token: Token,
    ready: &Sender<io::Result<S>>,
    stop: &AtomicBool,
    capacity: usize,
) where
    S: Proving + Send + Sync + 'static,
    for<'a> &'a S: Read + Write,
{
    let pending = Arc::new(Mutex::new(Admission::new(capacity)));
    for raw in incoming {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let raw = match raw {
            Ok(raw) => {
                errors.accepted();
                raw
            }
            Err(error) if crate::net::accept_error_concerns_one_connection(&error) => {
                match errors.failed() {
                    Ok(pause) => {
                        thread::sleep(pause);
                        continue;
                    }
                    Err(ended) => {
                        drop(ready.send(Err(ended)));
                        return;
                    }
                }
            }
            Err(error) => {
                // The listener is dead: hand the error to whoever accepts, then stop. If the
                // listener was dropped as well, nobody is left to hear it.
                drop(ready.send(Err(error)));
                return;
            }
        };
        let raw = Arc::new(raw);
        let Ok(mut waiting) = pending.lock() else {
            return;
        };
        let (number, evicted) = waiting.admit(Arc::clone(&raw));
        drop(waiting);
        if let Some(oldest) = evicted {
            oldest.close();
        }
        let (ready, pending) = (ready.clone(), Arc::clone(&pending));
        let spawned = thread::Builder::new()
            .name("local-handshake".into())
            .spawn(move || {
                let proved = endpoint::serve(&*raw, &token);
                if let Ok(mut waiting) = pending.lock() {
                    waiting.release(number);
                }
                if let Err(error) = &proved
                    && error.kind() == io::ErrorKind::PermissionDenied
                {
                    // A peer that answered and was wrong. One that hung up or was too slow is a
                    // probe or a client that gave up, and anyone who asks whether a program
                    // runs probes the same endpoint.
                    crate::log::event("local.proof_refused", &[]);
                }
                if proved.is_ok()
                    && let Ok(stream) = raw.duplicate()
                {
                    // The listener was dropped when this fails: nobody is left to take it.
                    drop(ready.send(Ok(stream)));
                }
            });
        if spawned.is_err() {
            // Out of threads: this connection is dropped, which its client sees as a reset.
            thread::sleep(Duration::from_millis(50));
        }
    }
}
