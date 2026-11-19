//! The replay of a subscription, fed to the connection as its queue drains (ADR 0168).
//!
//! A subscription's replay is up to a tail of output, and a client that subscribes many
//! terminals at once is owed many of them. Queueing them all at once would pass the connection's
//! queue and the engine would close a client that had only asked. Each connection keeps the
//! subscriptions it still owes a replay in the order they were made, and serves the first to its
//! end before the next, so a terminal is ready as soon as its own bytes are sent: a slice at a
//! time, each sent only while the queue holds `ROOM_LEVEL` bytes or fewer, and started again when
//! the writer has drained it (`Outbox::has_room_or_wake`). What a slice needs from the disk is read
//! by a job with no Core lock, one at a time for a connection.

use crate::{
    errors::Result,
    model::{self, Connected},
    streak::Streak,
};
use serde_json::json;
use soyeht_protocol::{event, limits};
use soyeht_store::segments::Reader;
use std::{collections::VecDeque, sync::Arc};

/// The bytes of output one slice of a replay carries.
pub(crate) const SLICE_BYTES: usize = limits::TERMINAL_PAYLOAD_BYTES * 4;
/// The bytes a connection's queue may hold, replay and all, once a slice is sent. The rest of
/// the queue is for what the client is owed live and for its replies.
pub(crate) const QUEUE_BYTES: usize = SLICE_BYTES * 4;
/// The level the queue falls to before the next slice is sent.
pub(crate) const ROOM_LEVEL: usize = QUEUE_BYTES - SLICE_BYTES;

/// The replays a connection still owes.
pub struct Replays {
    /// The subscriptions owed a slice (pane and binary reference), in the order they get it:
    /// the first is served to its end before the next.
    pub(crate) queue: VecDeque<(String, u32)>,
    /// A job reads the next slice of one of them from the disk.
    pub(crate) reading: bool,
    /// Reads of one slice that failed in a row, and when to log it.
    pub(crate) failures: Streak,
}

impl Default for Replays {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            reading: false,
            failures: Streak::once(),
        }
    }
}

impl Replays {
    pub fn add(&mut self, pane_id: &str, binary_ref: u32) {
        self.queue.push_back((pane_id.to_owned(), binary_ref));
    }

    pub(crate) fn wants_slice(&self) -> bool {
        !self.reading && !self.queue.is_empty()
    }
}

/// What a step of a replay left to do.
pub(crate) enum Slice {
    /// The slice was sent, or there was none to send.
    Sent,
    /// The slice must be read from the disk, from this position.
    Disk(Reader, u64),
}

/// After a slice: the replay is done, or the subscription stays first in line for its next.
pub(crate) fn settle(client: &mut Connected, pane_id: &str, binary_ref: u32) -> Result<()> {
    let Some(subscription) = client
        .streams
        .get_mut(pane_id)
        .filter(|s| s.binary_ref == binary_ref)
    else {
        return Ok(());
    };
    if subscription.next < subscription.replay_until {
        client
            .replays
            .queue
            .push_front((pane_id.to_owned(), binary_ref));
        return Ok(());
    }
    client.outbox.send_stream(
        model::notification(
            event::STREAM_REPLAY_DONE,
            json!({"streamId":subscription.stream_id,"nextOffset":subscription.next.to_string()}),
        )?,
        Arc::clone(&subscription.current),
    );
    subscription.replaying = false;
    Ok(())
}

#[cfg(test)]
mod tests;
