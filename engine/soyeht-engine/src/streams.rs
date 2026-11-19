use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    model::{self, Subscription},
    outbox::{CloseReason, Outbox},
    replay::{ROOM_LEVEL, SLICE_BYTES, Slice, settle},
    screen::ReplayRead,
};
use serde_json::{Value, json};
use soyeht_platform::log::{self, Failure as _};
use soyeht_protocol::{event, frame::Frame, limits, method};
use soyeht_store::segments::{Reader, Record};
use std::sync::{Arc, atomic::AtomicBool};

impl Core {
    /// Routes `pane.stream.subscribe` and `pane.stream.unsubscribe`.
    pub fn stream_request(
        &mut self,
        connection: &str,
        request_id: Value,
        method: &str,
        params: &Value,
    ) -> Result<Outcome> {
        if method == method::PANE_STREAM_SUBSCRIBE {
            self.subscribe_stream(connection, request_id, params)?;
            return Ok(Outcome::Queued);
        }
        let pane_id = errors::param_str(params, "paneId")?;
        errors::invariant(
            self.clients.get_mut(connection),
            "Stream connection disappeared",
        )?
        .streams
        .remove(pane_id);
        Ok(Outcome::Reply(json!({})))
    }

    fn subscribe_stream(&mut self, connection: &str, id: Value, params: &Value) -> Result<()> {
        let pane_id = errors::param_str(params, "paneId")?;
        let running = self
            .panes
            .get(pane_id)
            .filter(|p| !p.stored.closed)
            .ok_or_else(|| errors::not_found("Pane not found"))?;
        if running.stored.pane.mode != soyeht_protocol::PaneMode::Terminal {
            return Err(errors::invalid("Binary streams require a terminal pane"));
        }
        let stream_id = running
            .stored
            .pane
            .stream_id
            .clone()
            .ok_or_else(|| errors::internal("Terminal pane has no stream identity"))?;
        let generation = running.stored.pane.generation;
        let high = running.screen.next_offset();
        let tail = params["from"] == "tail";
        let from = if tail {
            running.screen.tail_start()
        } else {
            if params["from"]["streamId"] != stream_id {
                return Err(errors::resync_required("Stream identity changed"));
            }
            let offset = params["from"]["offset"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| errors::invalid("Invalid stream offset"))?;
            if offset < running.screen.first_offset() || offset > high {
                return Err(errors::resync_required(
                    "Stream offset outside retained range",
                ));
            }
            offset
        };
        let current = Arc::new(AtomicBool::new(true));
        let client = errors::invariant(
            self.clients.get_mut(connection),
            "Stream connection disappeared",
        )?;
        let binary_ref = client.next_binary_ref;
        client.next_binary_ref = binary_ref
            .checked_add(1)
            .ok_or_else(|| errors::resync_required("Connection stream references exhausted"))?;
        // Mapping precedes every binary frame. All tail boundaries were captured
        // while holding the same engine lock used by output ingestion.
        client.streams.remove(pane_id);
        client.outbox.send(model::response(method::PANE_STREAM_SUBSCRIBE,id,json!({"streamId":stream_id,"paneId":pane_id,"generation":generation,"firstRetainedOffset":running.screen.first_offset().to_string(),"nextOffset":high.to_string(),"binaryPaneRef":binary_ref}))?);
        client.outbox.send_stream(model::notification(event::STREAM_REPLAY_START,json!({"streamId":stream_id,"generation":generation,"fromOffset":from.to_string(),"toOffset":high.to_string(),"reset":tail}))?,Arc::clone(&current));
        client.streams.insert(
            pane_id.into(),
            Subscription {
                binary_ref,
                stream_id,
                generation,
                next: from,
                replay_until: high,
                replaying: true,
                current,
            },
        );
        client.replays.add(pane_id, binary_ref);
        Ok(())
    }

    /// Sends each subscriber what its pane has written since, and the replays that connections
    /// are owed as far as their queues have room (ADR 0168). It records whether a subscriber
    /// that is caught up is left behind (see `State::streams_behind`): the tick keeps its floor
    /// until they have caught up. A replay waits for the writer instead and is woken by it.
    pub fn pump_streams(&mut self) -> Result<()> {
        let fed = self.feed_replays();
        let mut behind = false;
        let pumped = self.pump_each_stream(&mut behind);
        self.note_streams_behind(behind);
        fed.and(pumped)
    }

    fn pump_each_stream(&mut self, behind: &mut bool) -> Result<()> {
        for client in self
            .clients
            .values_mut()
            .filter(|c| c.actor.is_some() && !c.outbox.is_closed())
        {
            let ids: Vec<_> = client
                .streams
                .iter()
                .filter(|(_, subscription)| !subscription.replaying)
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                let Some(pane) = self.panes.get(&id).filter(|p| !p.stored.closed) else {
                    client.streams.remove(&id);
                    continue;
                };
                let subscription = errors::invariant(
                    client.streams.get_mut(&id),
                    "Stream subscription disappeared during its pump",
                )?;
                if pane.stored.pane.stream_id.as_ref() != Some(&subscription.stream_id) {
                    client
                        .outbox
                        .close(crate::outbox::CloseReason::StreamChanged);
                    break; // Reconnect snapshot discovers the new stream generation.
                }
                let through = pane.screen.next_offset();
                if subscription.next < pane.screen.first_offset() {
                    let to = pane.screen.first_offset().min(through);
                    skip_to(&client.outbox, subscription, to)?;
                }
                let started_at = subscription.next;
                if subscription.next < through {
                    let records = pane.screen.read(
                        subscription.next,
                        through,
                        limits::TERMINAL_PAYLOAD_BYTES,
                    )?;
                    deliver(&client.outbox, subscription, records)?;
                }
                // Left short of what it was owed, after sending something or being refused: another
                // pump is due. One that sent nothing and was not refused has nothing more to send.
                *behind |= subscription.next < through && subscription.next != started_at;
            }
        }
        Ok(())
    }

    /// Sends the slices of replays that connections are owed, as far as their queues have room.
    pub(crate) fn feed_replays(&mut self) -> Result<()> {
        let owed: Vec<String> = self
            .clients
            .iter()
            .filter(|(_, c)| c.actor.is_some() && !c.outbox.is_closed() && c.replays.wants_slice())
            .map(|(id, _)| id.clone())
            .collect();
        let mut failed = Ok(());
        for connection in owed {
            if let Err(error) = self.feed_connection(&connection) {
                failed = failed.and(Err(error));
            }
        }
        failed
    }

    fn feed_connection(&mut self, connection: &str) -> Result<()> {
        let kick = self.background.kicker();
        loop {
            let Some(client) = self.clients.get_mut(connection) else {
                return Ok(());
            };
            if !client.replays.wants_slice()
                || client.outbox.is_closed()
                || !client.outbox.has_room_or_wake(ROOM_LEVEL, &kick)
            {
                return Ok(());
            }
            let Some((pane_id, binary_ref)) = client.replays.queue.pop_front() else {
                return Ok(());
            };
            // A step that fails has lost its place in the line: the connection ends and its client
            // starts over, rather than one terminal never being told its replay is done.
            let step = self.replay_slice(connection, &pane_id, binary_ref);
            match self.closing_on_failure(connection, step)? {
                Slice::Sent => {}
                Slice::Disk(reader, from) => {
                    let started =
                        self.read_from_disk(connection, pane_id, binary_ref, from, reader);
                    return self.closing_on_failure(connection, started);
                }
            }
        }
    }

    /// Ends the connection with `worker_failed` when `result` is an error, and returns it.
    fn closing_on_failure<T>(&self, connection: &str, result: Result<T>) -> Result<T> {
        if result.is_err()
            && let Some(client) = self.clients.get(connection)
        {
            client.outbox.close(CloseReason::WorkerFailed);
        }
        result
    }

    /// Sends the next slice of one replay that comes from memory, or says it comes from the disk.
    fn replay_slice(&mut self, connection: &str, pane_id: &str, binary_ref: u32) -> Result<Slice> {
        let Some(client) = self.clients.get_mut(connection) else {
            return Ok(Slice::Sent);
        };
        let Some(subscription) = client
            .streams
            .get_mut(pane_id)
            .filter(|s| s.binary_ref == binary_ref && s.replaying)
        else {
            // Unsubscribed, or subscribed again: this entry is stale.
            return Ok(Slice::Sent);
        };
        let Some(pane) = self.panes.get(pane_id).filter(|p| !p.stored.closed) else {
            client.streams.remove(pane_id);
            return Ok(Slice::Sent);
        };
        if pane.stored.pane.stream_id.as_ref() != Some(&subscription.stream_id) {
            client.outbox.close(CloseReason::StreamChanged);
            return Ok(Slice::Sent);
        }
        let until = subscription.replay_until;
        if subscription.next < pane.screen.first_offset() {
            let to = pane.screen.first_offset().min(until);
            skip_to(&client.outbox, subscription, to)?;
        }
        if subscription.next < until {
            let from = subscription.next;
            match pane.screen.replay_read(from, until, SLICE_BYTES)? {
                ReplayRead::Disk(reader) => return Ok(Slice::Disk(reader, from)),
                ReplayRead::Ready(records) => {
                    deliver(&client.outbox, subscription, records)?;
                    if subscription.next == from && !client.outbox.is_closed() {
                        return Err(errors::internal("A replay read made no progress"));
                    }
                }
            }
        }
        settle(client, pane_id, binary_ref)?;
        Ok(Slice::Sent)
    }

    /// Starts the job that reads a slice from the disk. Its result is applied by
    /// `send_disk_slice`, and the pump that follows sends the next one.
    fn read_from_disk(
        &mut self,
        connection: &str,
        pane_id: String,
        binary_ref: u32,
        from: u64,
        reader: Reader,
    ) -> Result<()> {
        let key = format!("replay:{connection}");
        if !self.background.accepts(&key) {
            return Err(errors::internal("A replay read is already running"));
        }
        if let Some(client) = self.clients.get_mut(connection) {
            client.replays.reading = true;
        }
        // A disk that hangs must not hold one of the shared workers.
        self.background.run_alone(&key);
        let (owner, pane) = (connection.to_owned(), pane_id.clone());
        let started = self.spawn_prompt_background(
            key,
            move || reader.read().map_err(errors::storage),
            move |core, read| {
                let applied = core.send_disk_slice(&owner, &pane, binary_ref, from, read);
                core.closing_on_failure(&owner, applied)
            },
            |_| true,
        );
        if started.is_err()
            && let Some(client) = self.clients.get_mut(connection)
        {
            client.replays.reading = false;
        }
        started
    }

    /// Sends what the job read, if its subscription is still waiting for exactly that.
    fn send_disk_slice(
        &mut self,
        connection: &str,
        pane_id: &str,
        binary_ref: u32,
        from: u64,
        read: Result<Vec<Record>>,
    ) -> Result<()> {
        let Some(client) = self.clients.get_mut(connection) else {
            return Ok(());
        };
        client.replays.reading = false;
        let Some(subscription) = client
            .streams
            .get_mut(pane_id)
            .filter(|s| s.binary_ref == binary_ref && s.replaying && s.next == from)
        else {
            // Unsubscribed, or subscribed again: the new subscription has its own entry.
            return Ok(());
        };
        let Some(pane) = self.panes.get(pane_id) else {
            // The next step finds the pane closed and lets the subscription go.
            client
                .replays
                .queue
                .push_front((pane_id.to_owned(), binary_ref));
            return Ok(());
        };
        let records = match read {
            Ok(records) if pane.stored.pane.stream_id.as_ref() == Some(&subscription.stream_id) => {
                client.replays.failures.succeed();
                records
            }
            // The pane runs another stream: the next step closes the connection.
            Ok(_) => Vec::new(),
            Err(error) => {
                // A segment that retention removed meanwhile is read again, from where the log now
                // begins; a disk that keeps failing ends the connection, and its client starts over.
                if client.replays.failures.fail(&error.failure_kind()) {
                    log::failure("engine.replay_read_failed", &error);
                }
                if client.replays.failures.attempts() > 1 {
                    client.outbox.close(CloseReason::WorkerFailed);
                    return Ok(());
                }
                Vec::new()
            }
        };
        deliver(&client.outbox, subscription, records)?;
        settle(client, pane_id, binary_ref)
    }
}

/// Sends `records` to the subscriber, moving its position past each one the connection accepts.
pub(crate) fn deliver(
    outbox: &Outbox,
    subscription: &mut Subscription,
    records: Vec<Record>,
) -> Result<()> {
    for record in records {
        let end = record.end();
        let frame = match record {
            Record::Data { offset, bytes } => Frame::Terminal {
                pane_ref: subscription.binary_ref,
                generation: subscription.generation,
                offset,
                bytes,
            },
            Record::Gap { from, to } => gap_notice(subscription, from, to)?,
        };
        if !outbox.send_stream(frame, Arc::clone(&subscription.current)) {
            break;
        }
        subscription.next = end;
    }
    Ok(())
}

fn gap_notice(subscription: &Subscription, from: u64, to: u64) -> Result<Frame> {
    model::notification(
        event::STREAM_GAP,
        json!({"streamId":subscription.stream_id,"fromOffset":from.to_string(),"toOffset":to.to_string()}),
    )
}

/// Tells the subscriber of the output that retention took, and moves its position past it.
fn skip_to(outbox: &Outbox, subscription: &mut Subscription, to: u64) -> Result<()> {
    let notice = gap_notice(subscription, subscription.next, to)?;
    outbox.send_stream(notice, Arc::clone(&subscription.current));
    subscription.next = to;
    Ok(())
}
