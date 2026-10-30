use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use sha2::{Digest, Sha256};
use soyeht_protocol::limits;
use soyeht_ptyd::wire::Request;
use soyeht_store::agents;
use uuid::Uuid;

pub fn receipt_pending(store: &impl StorePort, key: &str) -> Result<bool> {
    let Some(rest) = key.strip_prefix("acp:") else {
        return Ok(false);
    };
    let Some((operation, attempt)) = rest.rsplit_once(':') else {
        return Ok(false);
    };
    let Ok(attempt) = attempt.parse::<u32>() else {
        return Ok(false);
    };
    Ok(store
        .agent_outgoing(operation)
        .map_err(errors::storage)?
        .is_some_and(|row| row.attempt == attempt))
}

impl Core {
    /// Sends the next page of the agent's oldest unsent message, off the Core lock (ADR 0126): the
    /// bytes go to the supervisor in a job, and the page is made durable when the supervisor
    /// has accepted it, under the lock ([`Core::finish_outbox_page`]), which then sends the next.
    /// A page already on its way is left alone. The engine never sends a page twice with the
    /// same attempt: the supervisor answers the same operation key with the same receipt.
    pub fn flush_agent_outbox(&mut self, pane: &str) -> Result<()> {
        let job = format!("outbox:{pane}");
        if !self.background.accepts(&job) {
            return Ok(());
        }
        let Some(prior) = self
            .store
            .agent_outbox(pane)
            .map_err(errors::storage)?
            .into_iter()
            .next()
        else {
            return Ok(());
        };
        let saved = &self.panes[pane].stored;
        if saved.closed
            || saved.pane.generation != prior.generation
            || saved.instance_id.as_deref() != Some(&prior.instance_id)
            || saved.supervisor_id.as_deref() != Some(&prior.supervisor_id)
        {
            return Err(errors::resync_required(
                "Native delivery belongs to a lost process; reconciliation is required",
            ));
        }
        let start = prior.accepted_bytes as usize;
        let end = (start + limits::TERMINAL_PAYLOAD_BYTES).min(prior.bytes.len());
        let bytes = &prior.bytes[start..end];
        let operation_key = format!("acp:{}:{}", prior.operation_key, prior.attempt);
        if bytes.is_empty() {
            return self.finish_outbox_page(pane, prior, operation_key, 0, Ok(Some(0)));
        }
        let request = Request::Input {
            instance_id: prior.instance_id.clone(),
            bytes: STANDARD.encode(bytes),
            operation_key: operation_key.clone(),
            fingerprint: format!("{:x}", Sha256::digest(bytes)),
        };
        let (pane, attempted, instance) = (pane.to_owned(), bytes.len(), prior.instance_id.clone());
        self.start_input(job, &instance, request, move |core, accepted| {
            let settled = core.finish_outbox_page(&pane, prior, operation_key, attempted, accepted);
            core.settle_agent_poll(&pane, settled)
        })
    }

    /// Makes the page the supervisor accepted durable and sends the next one. A page of a message
    /// that is not the oldest unsent one any more (the process was replaced, the message
    /// answered) is dropped: the supervisor keeps the receipt until it is confirmed.
    fn finish_outbox_page(
        &mut self,
        pane: &str,
        prior: soyeht_store::agents::Outgoing,
        operation_key: String,
        attempted: usize,
        accepted: Result<Option<usize>>,
    ) -> Result<()> {
        let Some(accepted) = accepted? else {
            return Ok(());
        };
        if accepted > attempted {
            return Err(errors::ptyd_unavailable(
                "Native input receipt exceeds the attempted bytes",
            ));
        }
        let current = self
            .store
            .agent_outbox(pane)
            .map_err(errors::storage)?
            .into_iter()
            .next();
        if current.as_ref() != Some(&prior) {
            return Ok(());
        }
        StorePort::mutate(
            &mut self.store,
            "agent-delivery",
            &Uuid::new_v4().to_string(),
            "advance",
            |tx| {
                let next = agents::advance(tx, &prior, accepted)?;
                if next.accepted_bytes as usize == next.bytes.len() {
                    agents::delivered(tx, &next)?;
                }
                Ok((json!({}), vec![]))
            },
        )
        .map_err(errors::storage)?;
        // A crash before this confirmation leaves a stale ptyd receipt;
        // reconciliation sees the durable next attempt and can retire it. Nobody waits for it.
        self.send_later(Request::ConfirmInput { operation_key });
        if accepted == 0 {
            return Ok(());
        }
        self.flush_agent_outbox(pane)
    }
}
