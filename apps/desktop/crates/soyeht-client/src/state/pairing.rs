//! The pairing sheet's part of the replica: what the person asks of it, as the requests to send.

use super::{Connection, Effect, State};
use crate::{
    commands::Command,
    pairing::{Command as PairingCommand, Rejected},
};
use serde_json::Value;
use soyeht_protocol::Device;

fn send(command: PairingCommand) -> Effect {
    Effect::Send(Command::Pairing(command))
}

impl State {
    /// The person chose "Pair a phone": the sheet opens and a window is asked for.
    pub fn open_pairing(&mut self) -> Vec<Effect> {
        let connected = self.connection == Connection::Connected;
        self.pairing.open(connected).map(send).into_iter().collect()
    }

    /// What the person did with a request: allow with the code in `typed`, or deny. A decision
    /// that is not sent says why, and the request carries the words.
    pub fn decide_pairing(
        &mut self,
        request_id: &str,
        approve: bool,
        typed: &str,
    ) -> Result<Vec<Effect>, Rejected> {
        let command = self.pairing.decide(request_id, approve, typed)?;
        Ok(vec![send(command)])
    }

    /// The person confirmed the tailnet change a pairing was refused for.
    pub fn confirm_tailnet(&mut self) -> Vec<Effect> {
        let connected = self.connection == Connection::Connected;
        self.pairing
            .confirm_tailnet(connected)
            .map(send)
            .into_iter()
            .collect()
    }

    /// The person chose to revoke a device: the question comes first, and the engine is asked what
    /// the device made, so that the question can offer to close it.
    pub fn ask_to_revoke(&mut self, device_id: &str) -> Vec<Effect> {
        self.pairing
            .ask_to_revoke(device_id)
            .map(send)
            .into_iter()
            .collect()
    }

    /// The person confirmed revoking the device they were asked about, and whether to close the
    /// panes it made.
    pub fn confirm_revoke(&mut self, close_panes: bool) -> Vec<Effect> {
        self.pairing
            .confirm_revoke(close_panes)
            .map(send)
            .into_iter()
            .collect()
    }

    /// The person dismissed the notice of devices added.
    pub fn dismiss_notice(&mut self) -> Vec<Effect> {
        self.pairing
            .dismiss_notice()
            .map(send)
            .into_iter()
            .collect()
    }

    pub(super) fn adopt_pairing(
        &mut self,
        capabilities: &[Value],
        devices: &[Device],
        requests: &[Value],
    ) -> Vec<Effect> {
        let started = self
            .pairing
            .adopt(capabilities, devices.to_vec(), requests, true);
        // What the engine says of the devices added is read on each connection: the notice is the
        // engine's, and stays until it is dismissed.
        started
            .into_iter()
            .chain(self.pairing.load_notice())
            .map(send)
            .collect()
    }
}
