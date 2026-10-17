use crate::{
    errors::{self, Result},
    outbox::Outbox,
    ports::StorePort,
    screen::Screen,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Actor, ConfigValue, Pane, PaneClosedDraft, PaneConfigFailedDraft, PaneCreatedDraft,
    PanePermissionAnswerResult, PanePermissionAnsweredDraft, PaneQueuedDraft, PaneTurnEndedDraft,
    PaneUpdatedDraft, TurnStopReason, Workspace, attachment::AttachmentAssembler, frame::Frame,
};
use std::collections::BTreeMap;

pub use soyeht_store::domain::PaneRecord as StoredPane;
pub struct RunningPane {
    pub stored: StoredPane,
    pub screen: Screen,
}

/// A request answered later: the connection and id that asked, and the method whose result
/// it will get.
pub struct Waiter {
    pub connection: String,
    pub request_id: Value,
    pub method: String,
}

pub struct Subscription {
    pub binary_ref: u32,
    pub stream_id: String,
    pub generation: u32,
    pub next: u64,
    pub replay_until: u64,
    pub replaying: bool,
    pub current: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.current
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

pub struct Connected {
    pub outbox: Outbox,
    pub actor: Option<Actor>,
    pub scope: String,
    pub local: bool,
    pub signed_ui: bool,
    pub nonce: [u8; 32],
    pub events: bool,
    pub streams: BTreeMap<String, Subscription>,
    pub next_binary_ref: u32,
    pub pair_request: Option<String>,
    pub web_host: bool,
    pub attachments: AttachmentAssembler,
}

/// With `contract-checks`, an event or a result the contract does not describe is an
/// internal error and one line in the log. Without it nothing is checked.
#[cfg(feature = "contract-checks")]
fn conforms(
    subject: &str,
    mismatch: std::result::Result<(), soyeht_protocol::contract::Mismatch>,
) -> Result<()> {
    mismatch.map_err(|mismatch| {
        let reason = mismatch.to_string();
        soyeht_platform::log::event(
            "engine.contract_mismatch",
            &[("subject", subject), ("reason", &reason)],
        );
        errors::internal(format!("Contract mismatch: {reason}"))
    })
}

/// Checks the parameters of the event `method` (see [`conforms`]).
#[cfg(feature = "contract-checks")]
pub fn check_event(method: &str, params: &Value) -> Result<()> {
    conforms("event", soyeht_protocol::contract::event(method, params))
}

#[cfg(not(feature = "contract-checks"))]
pub fn check_event(_method: &str, _params: &Value) -> Result<()> {
    Ok(())
}

/// Checks the result of the method `method` (see [`conforms`]).
#[cfg(feature = "contract-checks")]
pub fn check_result(method: &str, result: &Value) -> Result<()> {
    conforms("result", soyeht_protocol::contract::result(method, result))
}

#[cfg(not(feature = "contract-checks"))]
pub fn check_result(_method: &str, _result: &Value) -> Result<()> {
    Ok(())
}

/// An event built from its generated type, as the store takes it (it adds the event id, the
/// time and the actor).
pub fn pending<P: soyeht_protocol::EventParams>(params: &P) -> Result<(String, Value)> {
    params
        .to_event()
        .map_err(|_| errors::internal("Event serialization failed"))
}

/// `pane.created` for a pane that has no document yet.
pub fn pane_created(pane: &Pane) -> Result<(String, Value)> {
    pending(&PaneCreatedDraft {
        pane: pane.clone(),
        file: None,
        writing_document: None,
    })
}

/// `pane.queued`: `n` prompts wait in the pane; `prompt_ids` are the ones this event names.
pub fn pane_queued(
    pane_id: &str,
    n: usize,
    prompt_ids: Vec<String>,
    draft: bool,
) -> Result<(String, Value)> {
    pending(&PaneQueuedDraft {
        pane_id: pane_id.into(),
        n: u32::try_from(n).map_err(|_| errors::internal("Queue length overflow"))?,
        prompt_ids,
        draft,
    })
}

/// `pane.permission_answered`: the answer to a permission request of `pane_id`.
pub fn permission_answered(
    pane_id: &str,
    answer: &PanePermissionAnswerResult,
) -> Result<(String, Value)> {
    pending(&PanePermissionAnsweredDraft {
        pane_id: pane_id.into(),
        request_id: answer.request_id.clone(),
        result: answer.result.clone(),
        option_id: answer.option_id.clone(),
        by: answer.by.clone(),
    })
}

/// `pane.turn_ended`: the turn of `prompt_id` ended for `stop_reason`.
pub fn turn_ended(
    pane_id: &str,
    prompt_id: String,
    stop_reason: TurnStopReason,
) -> Result<(String, Value)> {
    pending(&PaneTurnEndedDraft {
        pane_id: pane_id.into(),
        prompt_id,
        stop_reason,
    })
}

/// `pane.config_failed`: the selector `config_id` did not take `value`.
pub fn config_failed(
    pane_id: &str,
    config_id: String,
    value: ConfigValue,
    reason: &str,
) -> Result<(String, Value)> {
    pending(&PaneConfigFailedDraft {
        pane_id: pane_id.into(),
        config_id,
        value,
        reason: reason.into(),
    })
}

pub fn pane_updated(pane: &Pane) -> Result<(String, Value)> {
    pending(&PaneUpdatedDraft { pane: pane.clone() })
}

pub fn pane_closed(pane_id: &str, workspace_id: &str) -> Result<(String, Value)> {
    pending(&PaneClosedDraft {
        pane_id: pane_id.into(),
        workspace_id: workspace_id.into(),
    })
}

pub fn notification(method: &str, params: Value) -> Result<Frame> {
    check_event(method, &params)?;
    Ok(Frame::Json(errors::json_bytes(
        &json!({"jsonrpc":"2.0","method":method,"params":params}),
    )?))
}
pub fn response(method: &str, id: Value, result: Value) -> Result<Frame> {
    check_result(method, &result)?;
    Ok(Frame::Json(errors::json_bytes(
        &json!({"jsonrpc":"2.0","id":id,"result":result}),
    )?))
}
pub fn request(id: Value, method: &str, params: Value) -> Result<Frame> {
    Ok(Frame::Json(errors::json_bytes(
        &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )?))
}
pub fn failure(id: Value, error: soyeht_protocol::RpcError) -> Result<Frame> {
    Ok(Frame::Json(errors::json_bytes(
        &json!({"jsonrpc":"2.0","id":id,"error":error}),
    )?))
}

pub fn workspace(store: &impl StorePort, id: &str) -> Result<Workspace> {
    store
        .workspace(id)
        .map_err(errors::storage)?
        .ok_or_else(|| errors::not_found("Workspace not found"))
}

pub fn panes(store: &impl StorePort) -> Result<Vec<StoredPane>> {
    store.panes().map_err(errors::storage)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn_ended_event() -> Value {
        json!({
            "eventId": "1", "at": "2026-01-01T00:00:00Z",
            "paneId": "pane-1", "promptId": "prompt-1", "stopReason": "error"
        })
    }

    #[cfg(feature = "contract-checks")]
    #[test]
    fn an_event_or_result_the_contract_does_not_describe_is_an_error() {
        assert!(notification("pane.turn_ended", turn_ended_event()).is_ok());
        let mut misspelt = turn_ended_event();
        misspelt["paneID"] = misspelt["paneId"].take();
        assert!(notification("pane.turn_ended", misspelt).is_err());
        assert!(notification("pane.invented", json!({})).is_err());
        assert!(response("pane.close", json!(1), json!({"paneId": "pane-1"})).is_ok());
        assert!(response("pane.close", json!(1), json!({"pane": "pane-1"})).is_err());
    }

    #[cfg(not(feature = "contract-checks"))]
    #[test]
    fn without_the_checks_nothing_is_validated() {
        let mut misspelt = turn_ended_event();
        misspelt["paneID"] = misspelt["paneId"].take();
        assert!(notification("pane.turn_ended", misspelt).is_ok());
        assert!(response("pane.close", json!(1), json!({})).is_ok());
    }

    #[test]
    fn a_typed_event_carries_the_name_and_leaves_out_what_the_store_assigns() {
        let (name, params) =
            turn_ended("pane-1", "prompt-1".into(), TurnStopReason::Error).expect("typed event");
        assert_eq!(name, "pane.turn_ended");
        assert_eq!(
            params,
            json!({"paneId": "pane-1", "promptId": "prompt-1", "stopReason": "error"})
        );
    }
}
