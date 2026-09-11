use super::{Changes, state::Phase, transcript};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use soyeht_acp::{Message, rpc::Record};
use soyeht_protocol::{
    AttentionItem, PaneState, TranscriptKind, TranscriptRole, limits, ndjson::Decoder,
};
use soyeht_ptyd::wire::{Request, Response};

fn invalid(message: &str) -> crate::errors::Error {
    errors::harness_unavailable(message)
}

impl Core {
    /// Restore requests independently of raw retention. This never advances the
    /// stream checkpoint: the caller commits the declared gap only afterward.
    fn recover_agent_requests_before(&mut self, pane: &str, before: u64) -> Result<()> {
        let saved = self.panes[pane].stored.clone();
        let instance = errors::invariant(
            saved.instance_id.as_ref(),
            "Agent process has no adopted instance during recovery",
        )?;
        let Response::PendingRequests {
            items,
            recovery_gap,
        } = self.pty_request(Request::PendingRequests {
            instance_id: instance.clone(),
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
        for item in items.into_iter().filter(|item| item.from_seq < before) {
            total = total.saturating_add(item.bytes);
            if item.request_key != item.from_seq
                || item.bytes > limits::ACP_MESSAGE_BYTES
                || item.from_seq.checked_add(item.bytes as u64 + 1) != Some(item.to_seq)
                || total > limits::CONNECTION_QUEUE_BYTES
            {
                return Err(invalid("Invalid pending native request bounds"));
            }
            // Decisions and their outbox survive together. Existing requests
            // must keep their public ID, original deadline and first answer.
            if self
                .store
                .agent_request_at(pane, saved.pane.generation, item.from_seq)
                .map_err(errors::storage)?
                .is_some()
            {
                continue;
            }
            let mut bytes = Vec::with_capacity(item.bytes);
            while bytes.len() < item.bytes {
                let Response::PendingRequestChunk(chunk) =
                    self.pty_request(Request::PendingRequestChunk {
                        instance_id: instance.clone(),
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
        Ok(())
    }

    pub(crate) fn agent_output_gap(&mut self, pane: &str, to: u64) -> Result<()> {
        self.recover_agent_requests_before(pane, to)?;
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
        Ok(())
    }

    pub(crate) fn skip_interrupted_agent_record(
        &mut self,
        pane: &str,
        to: u64,
        complete: bool,
    ) -> Result<()> {
        self.recover_agent_requests_before(pane, to)?;
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
        Ok(())
    }
}
