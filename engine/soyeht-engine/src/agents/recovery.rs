use super::{Changes, state::Phase, transcript};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
    supervisor_handle::Supervisor,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use soyeht_acp::{Message, rpc::Record};
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
        let saved = self.panes[pane].stored.clone();
        let instance = errors::invariant(
            saved.instance_id.as_ref(),
            "Agent process has no adopted instance during recovery",
        )?;
        let fetched = self.reads.fetches.remove(pane);
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
                let requests = core.settle_supervisor(&used, requests);
                core.reads.fetches.insert(
                    owner,
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
        let sequence_base = self.panes[pane].stored.sequence_base;
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
        let mut saved = self.panes[pane].stored.clone();
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
            self.panes[pane].stored.clone(),
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
        assert_eq!(core.agents["agent"].decoder.next_offset(), 0);

        apply_until_done(&mut core, "recover:agent");
        assert!(core.agent_output_gap("agent", 5).unwrap());
        assert_eq!(core.agents["agent"].decoder.next_offset(), 5);
    }
}
