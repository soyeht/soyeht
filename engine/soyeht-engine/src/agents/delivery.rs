use crate::{
    core::Core,
    errors::{self, Result},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use sha2::{Digest, Sha256};
use soyeht_protocol::{ErrorKind, limits};
use soyeht_ptyd::wire::{Request, Response};
use soyeht_store::{agents, database::Store};
use uuid::Uuid;

pub fn receipt_pending(store: &Store, key: &str) -> soyeht_store::database::Result<bool> {
    let Some(rest) = key.strip_prefix("acp:") else {
        return Ok(false);
    };
    let Some((operation, attempt)) = rest.rsplit_once(':') else {
        return Ok(false);
    };
    let Ok(attempt) = attempt.parse::<u32>() else {
        return Ok(false);
    };
    Ok(agents::outgoing(store, operation)?.is_some_and(|row| row.attempt == attempt))
}

impl Core {
    pub fn flush_agent_outbox(&mut self, pane: &str) -> Result<()> {
        // A bounded amount of work per tick lets another pane cancel or answer
        // permission while this process accepts a large message in small writes.
        for _ in 0..8 {
            let Some(prior) = agents::outbox(&self.store, pane)
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
                return Err(errors::error(
                    ErrorKind::ResyncRequired,
                    "Native delivery belongs to a lost process; reconciliation is required",
                ));
            }
            let start = prior.accepted_bytes as usize;
            let end = (start + limits::TERMINAL_PAYLOAD_BYTES).min(prior.bytes.len());
            let bytes = &prior.bytes[start..end];
            let operation_key = format!("acp:{}:{}", prior.operation_key, prior.attempt);
            let accepted = if bytes.is_empty() {
                0
            } else {
                let Response::Accepted { bytes: accepted } = self.pty_request(Request::Input {
                    instance_id: prior.instance_id.clone(),
                    bytes: STANDARD.encode(bytes),
                    operation_key: operation_key.clone(),
                    fingerprint: format!("{:x}", Sha256::digest(bytes)),
                })?
                else {
                    return Err(errors::error(
                        ErrorKind::PtydUnavailable,
                        "Invalid native input receipt",
                    ));
                };
                if accepted > bytes.len() {
                    return Err(errors::error(
                        ErrorKind::PtydUnavailable,
                        "Native input receipt exceeds the attempted bytes",
                    ));
                }
                accepted
            };
            self.store
                .mutate(
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
            // reconciliation sees the durable next attempt and can retire it.
            self.pty_request(Request::ConfirmInput { operation_key })?;
            if accepted == 0 {
                break;
            }
        }
        Ok(())
    }
}
