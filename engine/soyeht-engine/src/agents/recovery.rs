use super::{
    Changes,
    batch::{self, Batch, Earlier},
    native_error,
    state::Phase,
    transcript,
};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
    supervisor_handle::Supervisor,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use soyeht_acp::{
    Message,
    rpc::{Record, decode},
};
use soyeht_protocol::{
    AttentionItem, PaneState, TranscriptKind, TranscriptRole, limits, ndjson::Decoder,
};
use soyeht_ptyd::wire::{Output, PendingRequest, Request, Response};
use soyeht_store::segments::Record as SegmentRecord;
use std::collections::BTreeSet;

fn invalid(message: &str) -> crate::errors::Error {
    errors::harness_unavailable(message)
}

/// A request the supervisor still holds for an agent, with its bytes.
type Recovered = (PendingRequest, Vec<u8>);

/// What a job read of the requests an agent had pending, for the poll that asked: the process,
/// the offset they were read up to, and the requests (with no bytes for the ones the engine has
/// already).
pub(crate) struct Fetched {
    instance_id: String,
    before: u64,
    requests: Result<Vec<Recovered>>,
}

/// Reads the requests `instance_id` has pending that begin before `before`, and the bytes of the
/// ones not in `known`. This waits for the supervisor: it runs off the Core lock.
fn fetch_pending(
    supervisor: &Supervisor,
    instance_id: &str,
    before: u64,
    known: &BTreeSet<u64>,
) -> Result<Vec<Recovered>> {
    let Response::PendingRequests {
        items,
        recovery_gap,
    } = supervisor.call(Request::PendingRequests {
        instance_id: instance_id.to_owned(),
    })?
    else {
        return Err(invalid("Invalid pending native request registry"));
    };
    if recovery_gap || items.len() > 128 {
        return Err(invalid(
            "Native request recovery is incomplete; the process must stop",
        ));
    }
    let mut total = 0usize;
    let mut found = Vec::new();
    for item in items.into_iter().filter(|item| item.from_seq < before) {
        total = total.saturating_add(item.bytes);
        if item.request_key != item.from_seq
            || item.bytes > limits::ACP_MESSAGE_BYTES
            || item.from_seq.checked_add(item.bytes as u64 + 1) != Some(item.to_seq)
            || total > limits::CONNECTION_QUEUE_BYTES
        {
            return Err(invalid("Invalid pending native request bounds"));
        }
        if known.contains(&item.from_seq) {
            found.push((item, Vec::new()));
            continue;
        }
        let mut bytes = Vec::with_capacity(item.bytes);
        while bytes.len() < item.bytes {
            let Response::PendingRequestChunk(chunk) =
                supervisor.call(Request::PendingRequestChunk {
                    instance_id: instance_id.to_owned(),
                    request_key: item.request_key,
                    offset: bytes.len(),
                })?
            else {
                return Err(invalid("Invalid pending native request page"));
            };
            let part = STANDARD
                .decode(chunk.bytes)
                .map_err(|_| invalid("Invalid native recovery encoding"))?;
            if chunk.request_key != item.request_key
                || chunk.offset != bytes.len()
                || chunk.total_bytes != item.bytes
                || part.is_empty()
                || part.len() > limits::TERMINAL_PAYLOAD_BYTES
                || chunk.next_offset != bytes.len() + part.len()
                || chunk.next_offset > item.bytes
            {
                return Err(invalid("Invalid pending native request page bounds"));
            }
            bytes.extend(part);
        }
        found.push((item, bytes));
    }
    Ok(found)
}

impl Core {
    /// Restore requests independently of raw retention. This never advances the
    /// stream checkpoint: the caller commits the declared gap only afterward. The supervisor is
    /// asked off the lock (ADR 0126): with the answer not here yet this starts the job and says
    /// `false`, and the caller leaves the gap for its next poll.
    fn recover_agent_requests_before(&mut self, pane: &str, before: u64) -> Result<bool> {
        let saved = self.pane(pane)?.stored.clone();
        let instance = errors::invariant(
            saved.instance_id.as_ref(),
            "Agent process has no adopted instance during recovery",
        )?;
        let fetched = self.reads.fetches.remove(&crate::read_ahead::agent(pane));
        if let Some(fetched) = fetched.filter(|f| f.instance_id == *instance && f.before == before)
        {
            return self.apply_recovered(pane, saved.pane.generation, fetched.requests?);
        }
        let job = format!("recover:{pane}");
        if !self.background.accepts(&job) {
            return Ok(false);
        }
        // The requests the engine has already need no bytes.
        let known: BTreeSet<u64> = self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?
            .into_iter()
            .filter(|request| request.generation == saved.pane.generation)
            .map(|request| request.source_seq)
            .collect();
        let supervisor = self.supervisor()?;
        let used = supervisor.clone();
        let (owner, asked, answered) = (pane.to_owned(), instance.clone(), instance.clone());
        self.spawn_prompt_background(
            job,
            move || fetch_pending(&supervisor, &asked, before, &known),
            move |core, requests| {
                // A failure of a link that was replaced meanwhile says nothing about the new one:
                // it is dropped, as an unfinished read is.
                let requests = match core.settle_if_current(&used, requests) {
                    Ok(Some(requests)) => Ok(requests),
                    Ok(None) => return Ok(()),
                    Err(error) => Err(error),
                };
                // A pane that closed meanwhile has nobody to take it.
                if core.panes.get(&owner).is_none_or(|pane| pane.stored.closed) {
                    return Ok(());
                }
                core.reads.fetches.insert(
                    crate::read_ahead::agent(&owner),
                    Fetched {
                        instance_id: answered,
                        before,
                        requests,
                    },
                );
                Ok(())
            },
            |_| true,
        )?;
        Ok(false)
    }

    /// Records the requests a job read: each one the engine does not have yet becomes a request
    /// of the agent, with its public identifier, its original deadline and its first answer.
    /// `false` when a request it needed the bytes of was recorded meanwhile and so was read
    /// without them: the caller asks again.
    fn apply_recovered(
        &mut self,
        pane: &str,
        generation: u32,
        requests: Vec<Recovered>,
    ) -> Result<bool> {
        for (item, bytes) in requests {
            // Decisions and their outbox survive together. Existing requests
            // must keep their public ID, original deadline and first answer.
            if self
                .store
                .agent_request_at(pane, generation, item.from_seq)
                .map_err(errors::storage)?
                .is_some()
            {
                continue;
            }
            if bytes.len() != item.bytes {
                return Ok(false);
            }
            let message = Message::parse(&bytes).map_err(super::native_error)?;
            if !matches!(&message, Message::Request { method, .. } if method == &item.method) {
                return Err(invalid("Pending native record is not the declared request"));
            }
            self.native_record(
                pane,
                Record {
                    from_seq: item.from_seq,
                    to_seq: item.to_seq,
                    message,
                },
                false,
            )?;
        }
        Ok(true)
    }

    /// Puts what a read of the agent's process found, from offset `seq`, into its screen: the
    /// gap the supervisor declared before it, if any, then the bytes.
    pub(crate) fn append_agent_output(
        &mut self,
        pane: &str,
        output: &Output,
        seq: u64,
    ) -> Result<()> {
        let sequence_base = self.pane(pane)?.stored.sequence_base;
        if output.from_seq != seq {
            if output.from_seq < seq
                || output
                    .gap
                    .as_ref()
                    .is_none_or(|gap| gap.from_seq != seq || gap.to_seq != output.from_seq)
            {
                return Err(errors::ptyd_unavailable("Invalid native output gap"));
            }
            self.panes
                .get_mut(pane)
                .ok_or_else(|| errors::internal("Agent pane disappeared during output gap"))?
                .screen
                .append(SegmentRecord::Gap {
                    from: sequence_base + seq,
                    to: sequence_base + output.from_seq,
                })?;
        }
        let bytes = STANDARD
            .decode(&output.bytes)
            .map_err(|_| errors::ptyd_unavailable("Invalid native output encoding"))?;
        if output.next_seq != output.from_seq + bytes.len() as u64 {
            return Err(errors::ptyd_unavailable(
                "Native output offsets disagree with bytes",
            ));
        }
        if !bytes.is_empty() {
            self.panes
                .get_mut(pane)
                .ok_or_else(|| errors::internal("Agent pane disappeared during output append"))?
                .screen
                .append(SegmentRecord::Data {
                    offset: sequence_base + output.from_seq,
                    bytes,
                })?;
        }
        Ok(())
    }

    /// Commits the gap before `to`. `false` while the requests it hides are being read.
    pub(crate) fn agent_output_gap(&mut self, pane: &str, to: u64) -> Result<bool> {
        if !self.recover_agent_requests_before(pane, to)? {
            return Ok(false);
        }
        let mut state = self.agent_state(pane)?;
        let mut saved = self.pane(pane)?.stored.clone();
        let mut changes = Changes {
            replay_messages: state.phase == Phase::Loading,
            ..Changes::default()
        };
        transcript::complete(&self.store, pane, &mut state, &mut changes)?;
        let notice = "Agent output contains a missing section. Pending requests were recovered; the interrupted protocol record is skipped.";
        changes.messages.push(transcript::new_message(
            &mut state,
            saved.pane.generation,
            TranscriptRole::System,
            TranscriptKind::System,
            notice.into(),
            true,
        )?);
        changes.attention.push(AttentionItem {
            attention_id: format!("{pane}:output-gap"),
            pane_id: pane.into(),
            request_id: None,
            message: notice.into(),
            created_at: self.timestamp(),
        });
        state.discard_until_newline = true;
        state.acknowledged_offset = to;
        if saved.pane.state != PaneState::WaitingForYou {
            saved.pane.state = PaneState::NoSignal;
            saved.pane.confidence = 0.0;
            saved.pane.phrase =
                "Agent output contains a missing section; turn completion is unconfirmed".into();
        }
        self.commit_agent(saved, state, changes, None, json!({}))?;
        errors::invariant(
            self.agents.get_mut(pane),
            "Agent runtime disappeared during gap recovery",
        )?
        .decoder = Decoder::new(to);
        Ok(true)
    }

    /// Skips the interrupted record before `to`. `false` while the requests it hides are being read.
    pub(crate) fn skip_interrupted_agent_record(
        &mut self,
        pane: &str,
        to: u64,
        complete: bool,
    ) -> Result<bool> {
        if !self.recover_agent_requests_before(pane, to)? {
            return Ok(false);
        }
        let mut state = self.agent_state(pane)?;
        state.acknowledged_offset = to;
        state.discard_until_newline = !complete;
        self.commit_agent(
            self.pane(pane)?.stored.clone(),
            state,
            Changes::default(),
            None,
            json!({}),
        )?;
        errors::invariant(
            self.agents.get_mut(pane),
            "Agent runtime disappeared while skipping an interrupted record",
        )?
        .decoder = Decoder::new(to);
        Ok(true)
    }
    /// Applies a record that came from the pane's own output. A `session/update` that changes
    /// only the transcript waits in `batch` for the commit of the poll; any other record ends
    /// the batch and is committed on its own, on top of what the batch left.
    fn apply_decoded(
        &mut self,
        pane: &str,
        record: Record,
        batch: &mut Option<Batch>,
    ) -> Result<()> {
        if !matches!(&record.message, Message::Notification { method, .. } if method == "session/update")
        {
            self.commit_batch(pane, batch)?;
            return self.native_record(pane, record, true);
        }
        let (mut state, mut saved, earlier) = match batch {
            Some(batch) => (batch.state.clone(), batch.saved.clone(), batch.earlier()),
            None => (
                self.agent_state(pane)?,
                self.pane(pane)?.stored.clone(),
                Earlier::default(),
            ),
        };
        let kept = Record {
            from_seq: record.from_seq,
            to_seq: record.to_seq,
            message: record.message.clone(),
        };
        let changes =
            self.apply_native_record(pane, record, true, (&mut state, &mut saved), earlier)?;
        match batch {
            Some(open) if open.takes(&changes) => open.absorb(changes, saved, state, kept),
            None if batch::transcript_only(&changes) => {
                *batch = Some(Batch::start(saved, state, changes, kept));
            }
            _ => {
                self.commit_batch(pane, batch)?;
                self.commit_agent(saved, state, changes, None, json!({}))?;
            }
        }
        Ok(())
    }

    /// Commits what the poll applied so far, as one transaction. If the store refuses the whole
    /// (a mutation the group could not foresee), the records are committed one by one, as they
    /// were before they were grouped, so the ones the store accepts are not lost with the one it
    /// refuses and the failure is the failing record's.
    fn commit_batch(&mut self, pane: &str, batch: &mut Option<Batch>) -> Result<()> {
        let Some((saved, state, changes, records)) = batch.take().map(Batch::finish) else {
            return Ok(());
        };
        let before = self.agents.require(pane)?.state.acknowledged_offset;
        match self.commit_agent(saved, state, changes, None, json!({})) {
            Err(errors::Error::Storage(_))
                if records.len() > 1 && !self.checkpoint_moved(pane, before) =>
            {
                records
                    .into_iter()
                    .try_for_each(|record| self.native_record(pane, record, true))
            }
            other => other.map(drop),
        }
    }

    /// Whether the stored checkpoint of the pane is past `before`: a commit that failed after
    /// the store took it must not be applied again.
    fn checkpoint_moved(&self, pane: &str, before: u64) -> bool {
        self.store
            .agent_runtime(pane)
            .ok()
            .flatten()
            .is_none_or(|stored| stored.acknowledged_offset != before)
    }

    /// First consume any raw records synced before an engine crash. The public
    /// checkpoint can lag the raw log, but it never advances beyond durable data. What one
    /// call decodes is committed once (ADR 0020): a crash before the commit leaves the
    /// checkpoint where it was, and the next start decodes the same raw bytes again.
    pub(super) fn decode_agent_log(&mut self, pane: &str) -> Result<()> {
        let base = self.pane(pane)?.stored.sequence_base;
        let offset = self.agents.require(pane)?.decoder.next_offset();
        let end = self.pane(pane)?.screen.next_offset();
        let mut from = base
            .checked_add(offset)
            .ok_or_else(|| errors::internal("Agent offset exhausted"))?;
        let first = self.pane(pane)?.screen.first_offset();
        if from < first {
            // The requests the gap hides are read off the lock; the log is read again next poll.
            if !self.agent_output_gap(pane, first - base)? {
                return Ok(());
            }
            from = first;
        }
        if from == end {
            return Ok(());
        }
        let records = self.pane(pane)?.screen.read(from, end, 64 * 1024)?;
        let mut batch = None;
        match self.decode_records(pane, base, records, &mut batch) {
            Ok(()) => match self.commit_batch(pane, &mut batch) {
                Ok(()) => Ok(()),
                Err(error) => self.abandon_decoding(pane, &mut None, error),
            },
            Err(error) => self.abandon_decoding(pane, &mut batch, error),
        }
    }

    /// Commits the records applied before a failure, so the decoder starts again after them, and
    /// returns the failure (or, when that commit failed too, its own, which leaves the decoder
    /// where the last commit left it and the records to be decoded again).
    fn abandon_decoding(
        &mut self,
        pane: &str,
        batch: &mut Option<Batch>,
        error: errors::Error,
    ) -> Result<()> {
        let committed = self.commit_batch(pane, batch);
        let running = errors::invariant(
            self.agents.get_mut(pane),
            "Agent runtime disappeared after a failure decoding output",
        )?;
        running.decoder = Decoder::new(running.state.acknowledged_offset);
        Err(committed.err().unwrap_or(error))
    }

    fn decode_records(
        &mut self,
        pane: &str,
        base: u64,
        records: Vec<SegmentRecord>,
        batch: &mut Option<Batch>,
    ) -> Result<()> {
        for record in records {
            let (mut offset, mut bytes) = match record {
                SegmentRecord::Gap { to, .. } => {
                    self.commit_batch(pane, batch)?;
                    if !self.agent_output_gap(pane, to - base)? {
                        return Ok(());
                    }
                    continue;
                }
                SegmentRecord::Data { offset, bytes } => (offset, bytes),
            };
            // The flag is only set by the recovery paths above and in `skip_interrupted_agent_record`,
            // which commit on their own after the group is committed, so the committed state is
            // the current one here even while a group of plain records is open.
            if self.agents.require(pane)?.state.discard_until_newline {
                let newline = bytes.iter().position(|byte| *byte == b'\n');
                let skipped = newline.map_or(bytes.len(), |at| at + 1);
                self.commit_batch(pane, batch)?;
                if !self.skip_interrupted_agent_record(
                    pane,
                    offset - base + skipped as u64,
                    newline.is_some(),
                )? {
                    return Ok(());
                }
                offset += skipped as u64;
                bytes.drain(..skipped);
                if bytes.is_empty() {
                    continue;
                }
            }
            let decoded = decode(
                &mut self
                    .agents
                    .get_mut(pane)
                    .ok_or_else(|| {
                        errors::internal("Agent runtime disappeared while decoding output")
                    })?
                    .decoder,
                offset - base,
                &bytes,
            )
            .map_err(native_error)?;
            for record in decoded {
                self.apply_decoded(pane, record, batch)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_panes::{Answering, apply_until_done, core_with_agent};
    use std::time::{Duration, Instant};

    #[test]
    fn the_requests_a_gap_hides_are_read_off_the_lock() {
        let (mut core, _directory) = core_with_agent(&[]);
        core.background = crate::offload::Background::default();
        core.link_supervisor(Box::new(Answering(|request| {
            Ok(match request {
                Request::PendingRequests { .. } => {
                    // A supervisor that takes its time to list what it holds.
                    std::thread::sleep(Duration::from_millis(400));
                    Response::PendingRequests {
                        items: Vec::new(),
                        recovery_gap: false,
                    }
                }
                _ => Response::Ok,
            })
        })));

        let began = Instant::now();
        assert!(!core.agent_output_gap("agent", 5).unwrap());
        // Asked again while the job runs, it waits for it and asks for nothing more.
        assert!(!core.agent_output_gap("agent", 5).unwrap());
        assert!(
            began.elapsed() < Duration::from_millis(250),
            "the recovery waited for the supervisor: {:?}",
            began.elapsed()
        );
        assert_eq!(core.agents.agent("agent").decoder.next_offset(), 0);

        apply_until_done(&mut core, "recover:agent");
        assert!(core.agent_output_gap("agent", 5).unwrap());
        assert_eq!(core.agents.agent("agent").decoder.next_offset(), 5);
    }

    /// A supervisor that takes `delay` to list the requests it holds, and then answers with `then`.
    fn listing(
        delay: Duration,
        then: fn() -> soyeht_ptyd::Result<Response>,
    ) -> Box<dyn crate::ports::SupervisorPort> {
        Box::new(Answering(move |request| match request {
            Request::PendingRequests { .. } => {
                std::thread::sleep(delay);
                then()
            }
            _ => Ok(Response::Ok),
        }))
    }

    fn nothing_pending() -> soyeht_ptyd::Result<Response> {
        Ok(Response::PendingRequests {
            items: Vec::new(),
            recovery_gap: false,
        })
    }

    fn refused() -> soyeht_ptyd::Result<Response> {
        Err(soyeht_ptyd::Error::Remote {
            kind: soyeht_ptyd::wire_kind::OPERATION_FAILED.into(),
            message: "no".into(),
        })
    }

    #[test]
    fn what_a_job_read_for_a_pane_that_closed_meanwhile_is_dropped() {
        let (mut core, _directory) = core_with_agent(&[]);
        core.background = crate::offload::Background::default();
        core.link_supervisor(listing(Duration::from_millis(200), nothing_pending));

        assert!(!core.agent_output_gap("agent", 5).unwrap());
        core.panes.get_mut("agent").unwrap().stored.closed = true;
        apply_until_done(&mut core, "recover:agent");
        assert!(core.reads.fetches.is_empty());
    }

    #[test]
    fn what_a_job_read_is_forgotten_with_the_reads_of_its_pane() {
        let (mut core, _directory) = core_with_agent(&[]);
        core.background = crate::offload::Background::default();
        core.link_supervisor(listing(Duration::ZERO, nothing_pending));

        assert!(!core.agent_output_gap("agent", 5).unwrap());
        apply_until_done(&mut core, "recover:agent");
        assert_eq!(core.reads.fetches.len(), 1);

        core.forget_reads(&crate::read_ahead::agent("agent"));
        assert!(core.reads.fetches.is_empty());
    }

    #[test]
    fn a_failure_of_a_link_that_was_replaced_is_not_kept_for_the_poll() {
        let (mut core, _directory) = core_with_agent(&[]);
        core.background = crate::offload::Background::default();
        core.link_supervisor(listing(Duration::from_millis(200), refused));
        assert!(!core.agent_output_gap("agent", 5).unwrap());
        // The link is replaced before the old one answers.
        core.link_supervisor(listing(Duration::ZERO, nothing_pending));
        apply_until_done(&mut core, "recover:agent");
        assert!(core.reads.fetches.is_empty());

        // The same failure on the link the engine has is kept, and the poll meets it.
        core.link_supervisor(listing(Duration::ZERO, refused));
        assert!(!core.agent_output_gap("agent", 5).unwrap());
        apply_until_done(&mut core, "recover:agent");
        assert_eq!(core.reads.fetches.len(), 1);
        assert!(core.agent_output_gap("agent", 5).is_err());
    }
}
