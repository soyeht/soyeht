//! What the pairing view asks of the engine, the typed requests that carry it out, and how each
//! answer, or each failure, comes back to the replica.

use super::{Event, Offer, Refusal};
use crate::{Error, Result, commands::key, event::Event as ClientEvent, rpc::Rpc, timestamp};
use soyeht_protocol::{
    DevicePairDecideParams, DevicePairDecideResult, DevicePairStartParams, DevicePairStartResult,
    DeviceRevokeParams, DeviceRevokeResult, Request, approval_code,
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
    Start {
        ticket: u64,
    },
    /// Allow or deny a request. `code` goes only to an engine that offers `pairing-code`, and only
    /// with an approval.
    Decide {
        request_id: String,
        approve: bool,
        code: Option<TypedCode>,
    },
    Revoke {
        device_id: String,
    },
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
            Self::Revoke { device_id } => Event::RevokeFailed {
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
        Command::Revoke { device_id } => {
            let revoked: DeviceRevokeResult =
                serde_json::from_value(rpc.call(&Request::DeviceRevoke(DeviceRevokeParams {
                    device_id: device_id.clone(),
                    idempotency_key: key(),
                }))?)?;
            Event::Revoked {
                device_id: revoked.device_id,
            }
        }
    };
    Ok(Some(ClientEvent::Pairing(event)))
}
