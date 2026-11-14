//! Pairing a phone with this computer (ADR 0164): the window the engine opens, the requests that
//! come in on it, the code the person types for each, and the devices already paired. It is a plain
//! value that applies the engine's events and the answers to its own commands, so every rule here
//! is testable without a window or a connection.
//!
//! The engine binds a pairing window to the connection that opened it and lets only that
//! connection decide the requests that come in on it. The client has one connection, so the sheet
//! keeps it for the whole pairing; when it is lost the window and its requests are lost with it,
//! and the pairing starts again on the next one ([`Pairing::adopt`]).

mod commands;
pub mod copy;
mod qr;
#[cfg(test)]
mod tests;

pub use commands::{Command, TypedCode, run};
pub use qr::{Matrix, QUIET_ZONE};

use crate::{Error, timestamp};
use serde_json::Value;
use soyeht_protocol::{
    Device, DevicePairPending, DevicePairRequestedEvent, DevicePairStartResult, DevicePairedEvent,
    DeviceRevokedEvent, EngineCapability, ErrorKind, approval_code, limits, pairing_link,
};

/// The window the engine opened for a phone to scan.
#[derive(Clone, PartialEq)]
pub struct Offer {
    /// The code to draw; none when the engine's text is longer than any QR code holds.
    pub matrix: Option<Matrix>,
    /// The link, for the person to copy.
    pub link: String,
    /// When the window ends, in seconds since the epoch.
    pub expires_at: f64,
    /// Why a phone cannot reach this computer, when it cannot.
    pub problem: Option<String>,
}

/// The window's code is the secret of the link: nothing prints it.
impl std::fmt::Debug for Offer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Offer")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Offer {
    /// The offer an engine's answer makes, at `now`. A window whose end the engine wrote in a form
    /// that cannot be read ends at the contract's length from now.
    pub fn of(started: DevicePairStartResult, now: f64) -> Self {
        let text = started.qr.as_deref().unwrap_or(&started.uri);
        Self {
            matrix: Matrix::encode(text),
            link: started.uri.clone(),
            expires_at: timestamp::seconds_since_epoch(&started.expires_at)
                .unwrap_or(now + limits::PAIRING_WINDOW_SECONDS as f64),
            problem: started
                .reachability
                .as_ref()
                .and_then(copy::reachability_problem),
        }
    }
}

/// Why the engine did not do what was asked, in its words.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    /// The engine's kind, none for a failure of the connection itself.
    pub kind: Option<ErrorKind>,
    pub message: String,
}

impl Refusal {
    pub fn of(error: &Error) -> Self {
        Self {
            kind: match error {
                Error::Remote(refusal) => Some(refusal.data.kind.clone()),
                _ => None,
            },
            message: error.to_string(),
        }
    }
}

/// Where the sheet is.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Phase {
    /// The sheet is closed. A window opened before may still be live on the engine.
    #[default]
    Closed,
    /// A window was asked for; `ticket` names the attempt.
    Starting {
        ticket: u64,
    },
    Showing(Offer),
    /// The engine did not open a window: where it cannot pair, its refusal says why.
    Refused(Refusal),
    /// The window ran out.
    Expired,
    /// The connection was lost with the sheet open: the window went with it.
    Interrupted,
}

/// A request to pair, waiting for the person.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub request_id: String,
    pub device_name: String,
    pub fingerprint: String,
    /// The short code both screens show (ADR 0152); none when the fingerprint has none, and such
    /// a request can only be denied.
    pub comparison: Option<String>,
    /// The answer to what the person did is awaited.
    pub deciding: bool,
    /// What to tell the person about it: why a code was not taken, or why the request is over.
    pub note: Option<String>,
    /// The request is over, so it offers nothing but to be dismissed.
    pub closed: bool,
    /// The codes the engine refused for this request. The engine denies the request at the last
    /// wrong one and tells the app nothing but the refusal, so the app counts them as it does.
    wrong_codes: u8,
}

impl Request {
    fn new(request_id: String, device_name: String, fingerprint: String) -> Self {
        Self {
            comparison: pairing_link::comparison_code(&fingerprint),
            request_id,
            device_name,
            fingerprint,
            deciding: false,
            note: None,
            closed: false,
            wrong_codes: 0,
        }
    }
}

/// Why a decision was not sent: nothing went to the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// There is no such request, or it is over, or its answer is awaited.
    Unavailable,
    /// The request has no short code to compare, so it cannot be allowed.
    NoComparison,
    /// What was typed is not six digits. The request says so; the engine is not asked.
    MalformedCode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Started {
        ticket: u64,
        offer: Offer,
    },
    StartFailed {
        ticket: u64,
        refusal: Refusal,
    },
    Decided {
        request_id: String,
        approved: bool,
    },
    DecideFailed {
        request_id: String,
        refusal: Refusal,
    },
    Revoked {
        device_id: String,
    },
    RevokeFailed {
        device_id: String,
        refusal: Refusal,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Pairing {
    /// The engine asks for the person's code (`pairing-code`).
    offers_code: bool,
    phase: Phase,
    requests: Vec<Request>,
    devices: Vec<Device>,
    /// The device the person is asked to confirm revoking.
    revoking: Option<String>,
    /// Why the last revocation failed.
    revoke_failure: Option<String>,
    tickets: u64,
}

impl Pairing {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The sheet is on screen.
    pub fn shown(&self) -> bool {
        self.phase != Phase::Closed
    }

    /// The engine asks the person for the code the phone shows.
    pub fn offers_code(&self) -> bool {
        self.offers_code
    }

    pub fn requests(&self) -> &[Request] {
        &self.requests
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    /// Opens the sheet and asks for a window; with no connection it waits for the next.
    pub fn open(&mut self, connected: bool) -> Option<Command> {
        if connected {
            self.tickets += 1;
            self.phase = Phase::Starting {
                ticket: self.tickets,
            };
            Some(Command::Start {
                ticket: self.tickets,
            })
        } else {
            self.phase = Phase::Interrupted;
            None
        }
    }

    /// Closes the sheet. A request that came in stays, and waits in the list of devices.
    pub fn close(&mut self) {
        self.phase = Phase::Closed;
    }

    /// The window runs out while the sheet shows it. True when that changed what is drawn.
    pub fn tick(&mut self, now: f64) -> bool {
        match &self.phase {
            Phase::Showing(offer) if now >= offer.expires_at => {
                self.phase = Phase::Expired;
                true
            }
            _ => false,
        }
    }

    /// The sheet shows a window that is running out: it is drawn again each second.
    pub fn counting(&self) -> bool {
        matches!(self.phase, Phase::Showing(_))
    }

    /// The whole seconds the window has left.
    pub fn remaining(&self, now: f64) -> Option<u64> {
        match &self.phase {
            Phase::Showing(offer) => Some((offer.expires_at - now).max(0.0).ceil() as u64),
            _ => None,
        }
    }

    /// The connection ended: the window and its requests were bound to it, so they are gone.
    pub fn connection_lost(&mut self) {
        self.requests.clear();
        self.revoking = None;
        if self.shown() {
            self.phase = Phase::Interrupted;
        }
    }

    /// A connection began, with what its snapshot says. A pairing the lost connection left
    /// interrupted starts again on this one.
    pub fn adopt(
        &mut self,
        capabilities: &[Value],
        devices: Vec<Device>,
        requests: &[Value],
        connected: bool,
    ) -> Option<Command> {
        let offered = serde_json::to_value(EngineCapability::PairingCode);
        self.offers_code = offered
            .as_ref()
            .is_ok_and(|offered| capabilities.contains(offered));
        self.devices = devices;
        // A request the engine lists in a form this build does not know is not a pairing to show.
        self.requests.clear();
        for value in requests {
            if let Ok(pending) = serde_json::from_value::<DevicePairPending>(value.clone()) {
                self.requests.push(Request::new(
                    pending.request_id,
                    pending.device_name,
                    pending.fingerprint,
                ));
            }
        }
        (self.phase == Phase::Interrupted)
            .then(|| self.open(connected))
            .flatten()
    }

    /// Takes in a notification about devices. False when `name` is not one.
    pub fn notified(
        &mut self,
        name: &soyeht_protocol::EventName,
        params: Value,
    ) -> serde_json::Result<bool> {
        use soyeht_protocol::EventName;
        match name {
            EventName::DevicePairRequested => {
                let event: DevicePairRequestedEvent = serde_json::from_value(params)?;
                self.requests.retain(|r| r.request_id != event.request_id);
                self.requests.push(Request::new(
                    event.request_id,
                    event.device_name,
                    event.fingerprint,
                ));
            }
            EventName::DevicePaired => {
                let paired: DevicePairedEvent = serde_json::from_value(params)?;
                self.requests
                    .retain(|r| r.fingerprint != paired.device.fingerprint);
                self.devices
                    .retain(|device| device.device_id != paired.device.device_id);
                self.devices.push(paired.device);
            }
            EventName::DeviceRevoked => {
                let revoked: DeviceRevokedEvent = serde_json::from_value(params)?;
                self.revoked(&revoked.device_id);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn revoked(&mut self, device_id: &str) {
        for device in self.devices.iter_mut().filter(|d| d.device_id == device_id) {
            device.revoked = true;
        }
        if self.revoking.as_deref() == Some(device_id) {
            self.revoking = None;
        }
    }

    /// What the person did with a request: the command that decides it, or why none is sent.
    /// `typed` is what is in the code field; it is read, and sent, only to an engine that asks for
    /// a code, and only to allow.
    pub fn decide(
        &mut self,
        request_id: &str,
        approve: bool,
        typed: &str,
    ) -> Result<Command, Rejected> {
        let offers_code = self.offers_code;
        let request = self
            .requests
            .iter_mut()
            .find(|request| {
                request.request_id == request_id && !request.closed && !request.deciding
            })
            .ok_or(Rejected::Unavailable)?;
        let mut code = None;
        if approve {
            if request.comparison.is_none() {
                return Err(Rejected::NoComparison);
            }
            if offers_code {
                let Some(digits) = approval_code::read(typed) else {
                    request.note = Some(copy::MALFORMED_CODE.to_owned());
                    return Err(Rejected::MalformedCode);
                };
                code = Some(TypedCode::new(digits));
            }
        }
        request.deciding = true;
        request.note = None;
        Ok(Command::Decide {
            request_id: request_id.to_owned(),
            approve,
            code,
        })
    }

    /// The person dismisses a request that is over.
    pub fn dismiss(&mut self, request_id: &str) {
        self.requests
            .retain(|request| !(request.request_id == request_id && request.closed));
    }

    /// The device the person is asked to confirm revoking.
    pub fn revoking(&self) -> Option<&Device> {
        let id = self.revoking.as_deref()?;
        self.devices.iter().find(|device| device.device_id == id)
    }

    pub fn revoke_failure(&self) -> Option<&str> {
        self.revoke_failure.as_deref()
    }

    /// The person asks to revoke a device: the question comes first.
    pub fn ask_to_revoke(&mut self, device_id: &str) {
        self.revoke_failure = None;
        if self
            .devices
            .iter()
            .any(|device| device.device_id == device_id && !device.revoked)
        {
            self.revoking = Some(device_id.to_owned());
        }
    }

    pub fn cancel_revoke(&mut self) {
        self.revoking = None;
    }

    /// The person confirmed.
    pub fn confirm_revoke(&mut self) -> Option<Command> {
        let device_id = self.revoking.take()?;
        Some(Command::Revoke { device_id })
    }

    /// Takes in what the engine answered to a command, or why it did not.
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Started { ticket, offer } if self.starting(ticket) => {
                self.phase = Phase::Showing(offer);
            }
            Event::StartFailed { ticket, refusal } if self.starting(ticket) => {
                self.phase = Phase::Refused(refusal);
            }
            // The answer of an attempt a newer one replaced, or of a sheet since closed.
            Event::Started { .. } | Event::StartFailed { .. } => {}
            Event::Decided {
                request_id,
                approved,
            } => {
                self.requests.retain(|r| r.request_id != request_id);
                // The person allowed it: the sheet has done its work, as the Mac's does.
                if approved && self.requests.is_empty() && self.shown() {
                    self.phase = Phase::Closed;
                }
            }
            Event::DecideFailed {
                request_id,
                refusal,
            } => self.decision_failed(&request_id, refusal),
            Event::Revoked { device_id } => self.revoked(&device_id),
            Event::RevokeFailed { refusal, .. } => {
                self.revoke_failure = Some(format!("{} {}", copy::REVOKE_FAILED, refusal.message));
            }
        }
    }

    fn starting(&self, ticket: u64) -> bool {
        self.phase == Phase::Starting { ticket }
    }

    /// A request the engine did not decide. Its refusal for a wrong code says how many tries are
    /// left, and the request stays; a request the engine no longer holds is over.
    fn decision_failed(&mut self, request_id: &str, refusal: Refusal) {
        let Some(request) = self
            .requests
            .iter_mut()
            .find(|r| r.request_id == request_id)
        else {
            return;
        };
        request.deciding = false;
        match refusal.kind {
            Some(ErrorKind::TransitionRefused | ErrorKind::NotFound | ErrorKind::Unauthorized) => {
                request.closed = true;
                request.note = Some(copy::REQUEST_GONE.to_owned());
            }
            Some(ErrorKind::PairingCodeInvalid) => {
                request.note = Some(refusal.message);
                request.wrong_codes += 1;
                // The engine has denied it: pressing Allow again could only be refused.
                request.closed = request.wrong_codes >= approval_code::WRONG_CODES_ALLOWED;
            }
            _ => request.note = Some(refusal.message),
        }
    }
}
