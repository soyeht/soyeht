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
    time::{Duration, Instant},
};

/// How long a listener waits after an accept the process had no descriptor for.
const OUT_OF_DESCRIPTORS_PAUSE: Duration = Duration::from_millis(100);

/// How often a listener that is still out of descriptors says so.
const EXHAUSTION_SUMMARY: Duration = Duration::from_secs(60);

/// The accepts that failed for lack of descriptors, in a row: the first is logged, then a summary
/// each minute with the count, and the accept that works again is logged once.
#[derive(Default)]
struct Exhaustion {
    /// When the run began and when it was last told, and how many accepts failed in it.
    run: Option<(Instant, Instant, u64)>,
}

impl Exhaustion {
    fn failed(&mut self) {
        self.failed_at(Instant::now());
    }

    fn failed_at(&mut self, now: Instant) {
        let (began, told, count) = self.run.get_or_insert_with(|| {
            crate::log::event("local.accept_out_of_descriptors", &[]);
            (now, now, 0)
        });
        *count += 1;
        if now.duration_since(*told) >= EXHAUSTION_SUMMARY {
            *told = now;
            crate::log::event(
                "local.accept_still_out_of_descriptors",
                &[
                    ("seconds", &now.duration_since(*began).as_secs().to_string()),
                    ("failed", &count.to_string()),
                ],
            );
        }
    }

    fn recovered(&mut self) {
        if let Some((began, _, count)) = self.run.take() {
            crate::log::event(
                "local.accept_recovered",
                &[
                    ("seconds", &began.elapsed().as_secs().to_string()),
                    ("failed", &count.to_string()),
                ],
            );
        }
    }
}

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
    let mut exhaustion = Exhaustion::default();
    for raw in incoming {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let raw = match raw {
            Ok(raw) => {
                errors.accepted();
                exhaustion.recovered();
                raw
            }
            Err(error) if crate::net::exhausts_resources(&error) => {
                // The process is out of descriptors, buffers or memory. The listener is fine, and
                // a connection or a pane that closes frees some: it waits and asks again, and
                // never ends, because what ends here is the supervisor and every terminal it
                // holds.
                exhaustion.failed();
                thread::sleep(OUT_OF_DESCRIPTORS_PAUSE);
                continue;
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
        let (ready, listed, held) = (ready.clone(), Arc::clone(&pending), Arc::clone(&raw));
        let spawned = thread::Builder::new()
            .name("local-handshake".into())
            .spawn(move || {
                let proved = endpoint::serve(&*held, &token);
                // Leave the admitted ones before anything else is done with the connection, so
                // that eviction, which closes what it finds there, cannot close one that proved.
                // One that was evicted first is closed already and is not handed on.
                let admitted = listed
                    .lock()
                    .is_ok_and(|mut waiting| waiting.release(number));
                if let Err(error) = &proved
                    && error.kind() == io::ErrorKind::PermissionDenied
                {
                    // A peer that answered and was wrong. One that hung up or was too slow is a
                    // probe or a client that gave up, and anyone who asks whether a program
                    // runs probes the same endpoint.
                    crate::log::event("local.proof_refused", &[]);
                }
                if proved.is_ok()
                    && admitted
                    && let Ok(stream) = held.duplicate()
                {
                    // The listener was dropped when this fails: nobody is left to take it.
                    drop(ready.send(Ok(stream)));
                }
            });
        if spawned.is_err() {
            // Out of threads: leave the admitted ones and close this connection, which its
            // client sees as a reset (the closure that held the other handle is gone).
            if let Ok(mut waiting) = pending.lock() {
                waiting.release(number);
            }
            raw.close();
            thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_without_descriptors_is_told_once_a_minute_and_ends_when_an_accept_works() {
        let start = Instant::now();
        let mut exhaustion = Exhaustion::default();
        exhaustion.failed_at(start);
        exhaustion.failed_at(start + Duration::from_secs(30));
        assert_eq!(exhaustion.run, Some((start, start, 2)));
        let minute = start + Duration::from_secs(61);
        exhaustion.failed_at(minute);
        assert_eq!(exhaustion.run, Some((start, minute, 3)));
        exhaustion.recovered();
        assert_eq!(exhaustion.run, None);
    }
}
