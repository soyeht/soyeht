//! What the pairing view asks of the engine, the typed requests that carry it out, and how each
//! answer, or each failure, comes back to the replica.

use super::{Event, Offer, Refusal};
use crate::{Error, Result, commands::key, event::Event as ClientEvent, rpc::Rpc, timestamp};
use soyeht_protocol::{
    DeviceNoticeDismissParams, DeviceNoticeDismissResult, DeviceNoticeListParams,
    DeviceNoticeListResult, DevicePairDecideParams, DevicePairDecideResult, DevicePairStartParams,
    DevicePairStartResult, DevicePanesListParams, DevicePanesListResult, DeviceRevokeParams,
    DeviceRevokeResult, PaneCloseParams, Request, approval_code,
};

/// The six digits the person typed for a request. They are the person's own action (ADR 0164), so
/// nothing prints them: not a log line, not a debug dump of the command that carries them.
#[derive(Clone, PartialEq, Eq)]
pub struct TypedCode(pub(super) String);

impl TypedCode {
    pub(super) fn new(digits: [u8; approval_code::DIGITS]) -> Self {
        Self(digits.iter().map(|digit| char::from(*digit)).collect())
    }
}

impl std::fmt::Debug for TypedCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TypedCode(..)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Open a pairing window. `ticket` names the attempt in the answer, so that the answer of an
    /// attempt a reconnect replaced is not taken for the current one.
    Start { ticket: u64 },
    /// Allow or deny a request. `code` goes only to an engine that offers `pairing-code`, and only
    /// with an approval.
    Decide {
        request_id: String,
        approve: bool,
        code: Option<TypedCode>,
    },
    /// Revoke a device and then close `close_panes`, the panes it made that the person chose to
    /// close: revoking does not stop what a device's terminals started (ADR 0164).
    Revoke {
        device_id: String,
        close_panes: Vec<String>,
    },
    /// Ask which devices were added since the notice was last dismissed.
    LoadNotice,
    /// Dismiss the notice up to the device added at `through`.
    DismissNotice { through: String },
    /// Ask which panes `device_id` made, for the question that precedes its revocation.
    LoadPanes { device_id: String },
}

impl Command {
    /// What the replica learns when this command fails: the engine's refusal in its own words, or
    /// why no answer came.
    pub fn failure(&self, error: &Error) -> Event {
        let refusal = Refusal::of(error);
        match self {
            Self::Start { ticket } => Event::StartFailed {
                ticket: *ticket,
                refusal,
            },
            Self::Decide { request_id, .. } => Event::DecideFailed {
                request_id: request_id.clone(),
                refusal,
            },
            Self::Revoke { device_id, .. } => Event::RevokeFailed {
                device_id: device_id.clone(),
                refusal,
            },
            Self::LoadNotice | Self::DismissNotice { .. } => Event::NoticeFailed { refusal },
            Self::LoadPanes { device_id } => Event::PanesFailed {
                device_id: device_id.clone(),
                refusal,
            },
        }
    }
}

/// Carries `command` out on the connection that is up. The window a start opens belongs to this
/// connection alone (ADR 0164): a request is decided on the connection that opened its window.
pub fn run(rpc: &Rpc, command: &Command) -> Result<Option<ClientEvent>> {
    let event = match command {
        Command::Start { ticket } => {
            let started: DevicePairStartResult = serde_json::from_value(rpc.call(
                &Request::DevicePairStart(DevicePairStartParams {
                    idempotency_key: key(),
                    // The tailnet-change confirmation is a later slice of ADR 0164.
                    confirm_tailnet: None,
                }),
            )?)?;
            Event::Started {
                ticket: *ticket,
                offer: Offer::of(started, timestamp::now()),
            }
        }
        Command::Decide {
            request_id,
            approve,
            code,
        } => {
            let decided: DevicePairDecideResult = serde_json::from_value(rpc.call(
                &Request::DevicePairDecide(DevicePairDecideParams {
                    request_id: request_id.clone(),
                    approve: *approve,
                    approval_code: code.as_ref().map(|code| code.0.clone()),
                    idempotency_key: key(),
                }),
            )?)?;
            Event::Decided {
                request_id: decided.request_id,
                approved: decided.approved,
            }
        }
        Command::Revoke {
            device_id,
            close_panes,
        } => {
            let revoked: DeviceRevokeResult =
                serde_json::from_value(rpc.call(&Request::DeviceRevoke(DeviceRevokeParams {
                    device_id: device_id.clone(),
                    idempotency_key: key(),
                }))?)?;
            // The device is revoked whatever happens to its panes: each one that cannot be closed
            // is counted, and the person is told how many are still open.
            let left_open = close_panes
                .iter()
                .filter(|pane_id| {
                    rpc.call(&Request::PaneClose(PaneCloseParams {
                        pane_id: (*pane_id).clone(),
                        idempotency_key: key(),
                    }))
                    .is_err()
                })
                .count();
            Event::Revoked {
                device_id: revoked.device_id,
                left_open,
            }
        }
        Command::LoadNotice => {
            let listed: DeviceNoticeListResult = serde_json::from_value(
                rpc.call(&Request::DeviceNoticeList(DeviceNoticeListParams {}))?,
            )?;
            Event::Noticed {
                devices: listed.items,
            }
        }
        Command::DismissNotice { through } => {
            let dismissed: DeviceNoticeDismissResult = serde_json::from_value(rpc.call(
                &Request::DeviceNoticeDismiss(DeviceNoticeDismissParams {
                    through: through.clone(),
                    idempotency_key: key(),
                }),
            )?)?;
            Event::NoticeDismissed {
                through: dismissed.through,
            }
        }
        Command::LoadPanes { device_id } => {
            let listed: DevicePanesListResult = serde_json::from_value(rpc.call(
                &Request::DevicePanesList(DevicePanesListParams {
                    device_id: device_id.clone(),
                }),
            )?)?;
            Event::PanesListed {
                device_id: device_id.clone(),
                panes: listed.items,
            }
        }
    };
    Ok(Some(ClientEvent::Pairing(event)))
}
