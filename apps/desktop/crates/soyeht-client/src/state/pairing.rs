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

    /// The person confirmed revoking the device they were asked about.
    pub fn confirm_revoke(&mut self) -> Vec<Effect> {
        self.pairing
            .confirm_revoke()
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
        self.pairing
            .adopt(capabilities, devices.to_vec(), requests, true)
            .map(send)
            .into_iter()
            .collect()
    }
}
