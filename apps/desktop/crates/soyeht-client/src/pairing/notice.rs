//! The part of the pairing state about what a person is told of the devices added to this
//! computer, and about the panes a device made when it is revoked (ADR 0164).
//!
//! Where the engine asks for the person's code it dates each device it pairs and names the node it
//! came from, so that adding one is never silent: the list says when and from where, and a notice
//! stays until the person dismisses it. Revoking a device does not stop what its terminals
//! started, so the question that precedes it lists the panes the device made.

use super::{Command, Pairing, Refusal, copy};
use crate::timestamp;
use soyeht_protocol::{Device, ErrorKind, Pane};

/// What is known of the panes a device made, for the question before its revocation.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DevicePanes {
    /// Asked for; the engine has not answered.
    #[default]
    Loading,
    Listed(Vec<Pane>),
    /// The engine could not say. The device can still be revoked.
    Unknown,
}

impl Pairing {
    /// The devices added since the person last dismissed the notice, oldest first.
    pub fn notice(&self) -> &[Device] {
        &self.notice
    }

    /// Why the notice could not be read or dismissed, when that is worth saying.
    pub fn notice_failure(&self) -> Option<&str> {
        self.notice_failure.as_deref()
    }

    /// The panes of the device the person is asked to confirm revoking.
    pub fn revoking_panes(&self) -> &DevicePanes {
        &self.revoking_panes
    }

    /// What the engine is asked, once connected, to tell the person of devices added. Only an
    /// engine that asks for the person's code dates devices, so only it has anything to say.
    pub fn load_notice(&self) -> Option<Command> {
        self.offers_code.then_some(Command::LoadNotice)
    }

    /// The person dismisses the notice, through the newest device it showed.
    pub fn dismiss_notice(&mut self) -> Option<Command> {
        let through = self
            .notice
            .iter()
            .filter_map(|device| device.added_at.as_deref())
            .max_by(|a, b| seconds(a).total_cmp(&seconds(b)))?
            .to_owned();
        self.notice_failure = None;
        Some(Command::DismissNotice { through })
    }

    /// A device was paired while the window is open: it is news at once, as the engine will say
    /// when it is asked.
    pub(super) fn added(&mut self, device: &Device) {
        if !self.offers_code || device.added_at.is_none() {
            return;
        }
        self.notice
            .retain(|seen| seen.device_id != device.device_id);
        self.notice.push(device.clone());
    }

    pub(super) fn notice_listed(&mut self, devices: Vec<Device>) {
        self.notice_failure = None;
        self.notice = devices;
    }

    pub(super) fn notice_dismissed(&mut self, through: &str) {
        let limit = seconds(through);
        self.notice.retain(|device| {
            device
                .added_at
                .as_deref()
                .is_some_and(|at| seconds(at) > limit)
        });
    }

    /// A refusal of the notice. An engine older than the notice does not know its calls; it has
    /// nothing to say, so the person is not told it failed.
    pub(super) fn notice_failed(&mut self, refusal: &Refusal) {
        if refusal.kind != Some(ErrorKind::InvalidParams) {
            self.notice_failure = Some(format!("{} {}", copy::NOTICE_FAILED, refusal.message));
        }
    }

    pub(super) fn panes_listed(&mut self, device_id: &str, panes: Vec<Pane>) {
        if self.revoking.as_deref() == Some(device_id) {
            self.revoking_panes = DevicePanes::Listed(panes);
        }
    }

    pub(super) fn panes_failed(&mut self, device_id: &str) {
        if self.revoking.as_deref() == Some(device_id) {
            self.revoking_panes = DevicePanes::Unknown;
        }
    }

    /// The person asks to revoke a device: the question comes first, and what the device made is
    /// asked of the engine so that the question can offer to close it.
    pub fn ask_to_revoke(&mut self, device_id: &str) -> Option<Command> {
        self.revoke_failure = None;
        if !self
            .devices
            .iter()
            .any(|device| device.device_id == device_id && !device.revoked)
        {
            return None;
        }
        self.revoking = Some(device_id.to_owned());
        // An engine that does not date devices is older than the list of panes.
        if self.offers_code {
            self.revoking_panes = DevicePanes::Loading;
            Some(Command::LoadPanes {
                device_id: device_id.to_owned(),
            })
        } else {
            self.revoking_panes = DevicePanes::Listed(Vec::new());
            None
        }
    }

    /// The person confirmed. With `close_panes` the panes the device made are closed after it is
    /// revoked, as the engine then lists them. Only the question that offered it can ask for it:
    /// one with no panes listed (or not yet) asks for none.
    pub fn confirm_revoke(&mut self, close_panes: bool) -> Option<Command> {
        let device_id = self.revoking.take()?;
        let close_panes = close_panes
            && matches!(&self.revoking_panes, DevicePanes::Listed(panes) if !panes.is_empty());
        Some(Command::Revoke {
            device_id,
            close_panes,
        })
    }
}

/// Seconds since the epoch of the engine's time; one the engine could not have written sorts first.
fn seconds(text: &str) -> f64 {
    timestamp::seconds_since_epoch(text).unwrap_or(f64::MIN)
}
