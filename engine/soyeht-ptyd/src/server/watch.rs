//! The watch role: a connection that only receives notices, so the engine reads a process when it
//! has output and not on a timer (ADR 0147, step 1a).
//!
//! The pseudoterminal reader of a process never writes to a socket: it marks the process in each
//! watch's pending set and wakes that watch's writer thread, which alone owns the socket. A slow
//! engine can therefore delay only its own notices, and the 5 s write timeout can stall no shell.

use super::{Live, State, lock, refuse, send};
use crate::{
    Result,
    wire::{FEATURE_WATCH, Readable, Request, Response, VERSION, Watched},
    wire_kind,
};
use soyeht_platform::{local::Stream, log};
use soyeht_protocol::frame::{read_json, write_json};
use std::{
    collections::{BTreeMap, VecDeque},
    net::Shutdown,
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

/// How long a notice waits to be written to a watch that does not read.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// How many watch connections the supervisor keeps. The engine opens one; a newer one takes the
/// place of the oldest, so a client that opens them in a loop cannot hold the supervisor's threads.
const MAX_WATCHES: usize = 4;
/// How many answers may wait to be written to one watch before it is closed. An engine sends one
/// request at a time and reads its answer, so a watch with a queue of them does not read.
const MAX_REPLIES: usize = 16;

#[derive(Default)]
struct Pending {
    /// Notices not yet written, one per process: a newer one replaces an older.
    notices: BTreeMap<String, Readable>,
    /// Answers to requests of the watch, written before the notices.
    replies: VecDeque<Response>,
    closed: bool,
}

struct Watch {
    pending: Mutex<Pending>,
    wake: Condvar,
    stream: Mutex<Option<Stream>>,
}

impl Watch {
    fn change<T>(&self, change: impl FnOnce(&mut Pending) -> T) -> T {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let result = change(&mut pending);
        self.wake.notify_all();
        result
    }

    /// Queues an answer to be written. Returns whether the queue is over [`MAX_REPLIES`]: the
    /// watch does not read, and its owner closes it.
    fn reply(&self, reply: Response) -> bool {
        self.change(|pending| {
            pending.replies.push_back(reply);
            pending.replies.len() > MAX_REPLIES
        })
    }

    /// Ends the watch: its writer stops and the read of its connection fails.
    fn close(&self) {
        self.change(|pending| pending.closed = true);
        let stream = self
            .stream
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(stream) = stream {
            // ignored: the peer may have closed it already; either way the watch ends here.
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl Live {
    /// The process has exited and its last byte has been read.
    pub(super) fn exited_and_drained(&self) -> bool {
        self.exited.load(Ordering::SeqCst) && self.read_done.load(Ordering::SeqCst)
    }

    /// Records that the process has exited, and tells the watches if its output is over too.
    /// Every place that sets the flag goes through here, so the exit is never left unannounced.
    pub(super) fn mark_exited(&self) {
        self.exited.store(true, Ordering::SeqCst);
        self.announce_exit();
    }

    /// Records that the last byte of the process's output has been read, and tells the watches
    /// if it has exited too.
    pub(super) fn mark_read_done(&self) {
        self.read_done.store(true, Ordering::SeqCst);
        self.announce_exit();
    }

    /// Tells the watches the process is over, once, when both its exit and the end of its output
    /// are known: the engine must not read an exit before the last byte. The two flags are stored
    /// and loaded with sequential consistency: the thread that sets the second of them sees the
    /// first, so one of the two callers always announces.
    fn announce_exit(&self) {
        if !self.exited_and_drained() || self.exit_announced.swap(true, Ordering::AcqRel) {
            return;
        }
        let end_seq = self
            .output
            .lock()
            .map_or(0, |output| output.retention.end_seq());
        self.watchers.announce(&Readable {
            instance_id: self.instance_id.clone(),
            end_seq,
            exited: true,
        });
    }
}

/// Why a hello of either role is turned away before the role matters: the protocol is not this
/// supervisor's, or the engine generation is lower than the recorded one. It reads the registry
/// and never changes it, and it runs before anything is closed or saved, so a refused hello
/// leaves everything as it was.
pub(super) fn hello_refusal(
    state: &State,
    version: u32,
    engine_generation: u64,
) -> Option<(&'static str, &'static str)> {
    if version != VERSION {
        return Some((
            wire_kind::VERSION_UNSUPPORTED,
            "incompatible supervisor protocol",
        ));
    }
    // No upper bound (ADR 0160): the engine takes its number at each start, before it reaches the
    // supervisor, so a crash loop can leave any distance, and a peer that has not proved the secret
    // never gets here.
    (engine_generation < state.registry.engine_generation).then_some((
        wire_kind::STALE_ENGINE_GENERATION,
        "engine generation is stale",
    ))
}

/// The welcome of a connection that was let in.
pub(super) fn welcome(state: &State) -> Response {
    Response::Welcome {
        version: VERSION,
        supervisor_id: state.supervisor_id.clone(),
        supervisor_generation: state.registry.supervisor_generation,
        features: vec![FEATURE_WATCH.into()],
    }
}

/// The open watch connections.
#[derive(Default)]
pub(super) struct Watchers {
    list: Mutex<Vec<Arc<Watch>>>,
    count: AtomicUsize,
}

impl Watchers {
    /// Whether any engine watches. A reader that finds none sends nothing and keeps no flag.
    pub(super) fn any(&self) -> bool {
        self.count.load(Ordering::Acquire) > 0
    }

    fn add(&self, watch: &Arc<Watch>) {
        let mut list = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        if list.len() >= MAX_WATCHES {
            list.remove(0).close();
        }
        list.push(Arc::clone(watch));
        self.count.store(list.len(), Ordering::Release);
    }

    fn remove(&self, watch: &Arc<Watch>) {
        let mut list = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        list.retain(|other| !Arc::ptr_eq(other, watch));
        self.count.store(list.len(), Ordering::Release);
    }

    /// Tells every watch that `notice.instance_id` has something to read.
    pub(super) fn announce(&self, notice: &Readable) {
        let list = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        for watch in list.iter() {
            watch.change(|pending| {
                let entry = pending
                    .notices
                    .entry(notice.instance_id.clone())
                    .or_insert_with(|| notice.clone());
                entry.end_seq = entry.end_seq.max(notice.end_seq);
                entry.exited |= notice.exited;
            });
        }
    }

    /// Closes every watch: the request connection they followed was replaced by a newer
    /// generation, and the new engine watches again with its own.
    pub(super) fn close_all(&self) {
        let watches =
            std::mem::take(&mut *self.list.lock().unwrap_or_else(PoisonError::into_inner));
        self.count.store(0, Ordering::Release);
        for watch in watches {
            watch.close();
        }
    }
}

/// Serves a watch connection whose hello has been read. The role binds to the engine generation
/// the supervisor has recorded and must equal it; it never writes the registry and never takes
/// the place of the request connection.
pub(super) fn serve(
    mut stream: Stream,
    shared: &Arc<Mutex<State>>,
    version: u32,
    engine_generation: u64,
) -> Result<()> {
    let watchers = {
        let state = lock(shared, "state")?;
        if let Some((kind, message)) = hello_refusal(&state, version, engine_generation) {
            return refuse(&mut stream, kind, message);
        }
        if engine_generation > state.registry.engine_generation {
            return refuse(
                &mut stream,
                wire_kind::UNAUTHORIZED,
                "no engine of this generation is connected",
            );
        }
        send(&mut stream, &welcome(&state))?;
        Arc::clone(&state.watchers)
    };
    let watch = Arc::new(Watch {
        pending: Mutex::default(),
        wake: Condvar::new(),
        stream: Mutex::new(Some(stream.try_clone()?)),
    });
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    watchers.add(&watch);
    write_notices(stream.try_clone()?, Arc::clone(&watch));
    let result = answer_requests(&mut stream, shared, &watch);
    watch.close();
    watchers.remove(&watch);
    result
}

fn answer_requests(
    stream: &mut Stream,
    shared: &Arc<Mutex<State>>,
    watch: &Arc<Watch>,
) -> Result<()> {
    while let Some(request) = read_json::<Request>(&mut *stream)? {
        let reply = match request {
            Request::Watch { instances } => Response::Watching(level(shared, &instances)?),
            _ => super::error(
                wire_kind::OPERATION_FAILED,
                "a watch connection only watches",
            ),
        };
        if watch.reply(reply) {
            return Ok(());
        }
    }
    Ok(())
}

/// Level before edge: the processes the engine has not read to the end of (all of them, for ones
/// it does not name), and the ones that have exited, so what happened before the watch (or between two watches) is not lost. The
/// "told" flag of every process is cleared, so the next output is announced.
fn level(shared: &Arc<Mutex<State>>, instances: &[Watched]) -> Result<Vec<Readable>> {
    let state = lock(shared, "state")?;
    let mut readable = Vec::new();
    for (instance_id, live) in &state.live {
        let mut output = lock(&live.output, "output")?;
        output.told = false;
        let end_seq = output.retention.end_seq();
        drop(output);
        // A process the engine did not name is one it has read nothing of.
        let from_seq = instances
            .iter()
            .find(|watched| watched.instance_id == *instance_id)
            .map_or(0, |watched| watched.from_seq);
        let exited = live.exited_and_drained();
        if from_seq < end_seq || exited {
            readable.push(Readable {
                instance_id: instance_id.clone(),
                end_seq,
                exited,
            });
        }
    }
    Ok(readable)
}

fn write_notices(mut stream: Stream, watch: Arc<Watch>) {
    thread::spawn(move || {
        loop {
            let (replies, notices) = {
                let mut pending = watch.pending.lock().unwrap_or_else(PoisonError::into_inner);
                while !pending.closed && pending.replies.is_empty() && pending.notices.is_empty() {
                    pending = watch
                        .wake
                        .wait(pending)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if pending.closed {
                    return;
                }
                (
                    std::mem::take(&mut pending.replies),
                    std::mem::take(&mut pending.notices),
                )
            };
            let frames = replies
                .into_iter()
                .chain(notices.into_values().map(Response::Readable));
            for frame in frames {
                if let Err(error) = write_json(&mut stream, &frame).map_err(crate::Error::from) {
                    log::failure("ptyd.watch_write_failed", &error);
                    watch.close();
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_with_a_queue_of_answers_is_over_the_limit_at_the_seventeenth() {
        let watch = Watch {
            pending: Mutex::default(),
            wake: Condvar::new(),
            stream: Mutex::new(None),
        };
        for _ in 0..MAX_REPLIES {
            assert!(!watch.reply(Response::Ok));
        }
        let over = watch.reply(Response::Ok);
        assert!(over);
    }
}
