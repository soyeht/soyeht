use crate::{
    auth,
    core::Core,
    errors::{self, Result},
    model,
    network::TailnetUpdate,
    pairing_approval::{Approval, Verdict, check_device_name},
    ports::{Clock, StorePort},
    ui_attestation,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_platform::log;
use soyeht_protocol::{
    Device, DevicePairDecideParams, DevicePairDecideResult, DevicePairPending,
    DevicePairRequestedDraft, DevicePairStartResult, DevicePairedDraft, DeviceRevokeParams,
    DeviceRevokeResult, DeviceRevokedDraft, PairBeginParams, PairBeginResult, PairProveParams,
    PairProveResult, PairResultEvent, Role, event, limits, method, pairing_link,
};
use std::sync::Once;
use uuid::Uuid;

/// A pairing request as the store keeps it. It holds no approval code: that lives in memory only
/// (`pairing_approval`).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct Pending {
    pub(crate) id: String,
    /// The code of the window the request came in on, not the person's approval code.
    pub(crate) code: String,
    name: String,
    key: String,
    challenge: [u8; 32],
    proved: bool,
    pub(crate) approved: Option<bool>,
    token: Option<String>,
    device: Option<Device>,
    pub(crate) expires: i64,
}

fn active_pending<S: StorePort, C: Clock + ?Sized>(
    store: &S,
    clock: &C,
    id: &str,
) -> Result<Pending> {
    store
        .active_pair_request(id, clock.now_unix())
        .map_err(errors::storage)?
        .ok_or_else(|| errors::unauthorized("Pairing request missing or expired"))
}

/// The refusal of a window, a decision or a revocation. When a local human is refused because this engine
/// does not carry our signature, it cannot tell the signed app from any other program, so even the
/// app is refused: say so, and log it once with its kind only (docs/dev-local.md says how to sign
/// the engine). On a system that attests nothing the app cannot be told from a shell either, and
/// the refusal says that the app cannot yet prove itself there (ADR 0162).
fn refusal(local_human: bool, approval: Approval) -> errors::Error {
    static LOGGED: Once = Once::new();
    // No signature to blame and nothing to log: this is the system's rule, not an engine fault.
    if local_human && (approval == Approval::Refused || !ui_attestation::available()) {
        return errors::forbidden_role(
            "Pairing is started and approved, and other devices are revoked, only in the signed Soyeht app, and on this system the app cannot yet prove itself to the engine, so no pairing is started or approved and no device is revoked here",
        );
    }
    if local_human && ui_attestation::engine_cannot_attest() {
        LOGGED.call_once(|| {
            log::event(
                "engine.attestation_unavailable",
                &[("reason", "engine_not_signed")],
            );
        });
        return errors::forbidden_role(
            "This engine does not carry the Soyeht signature, so it cannot tell the Soyeht app from other programs and starts no pairing, approves none and revokes no device; install the signed engine (docs/dev-local.md)",
        );
    }
    errors::forbidden_role(
        "Pairing is started and approved, and other devices are revoked, in the Soyeht app on this computer",
    )
}

/// What a device reads when its request was denied where the person is.
fn denial_text(approval: Approval) -> &'static str {
    if approval.asks_for_code() {
        "Denied on the computer"
    } else {
        "Denied on Mac"
    }
}

impl Core {
    fn pairing_client(&self, connection: &str) -> Result<&model::Connected> {
        errors::invariant(
            self.clients.get(connection),
            "Pairing connection disappeared",
        )
    }

    fn pairing_client_mut(&mut self, connection: &str) -> Result<&mut model::Connected> {
        errors::invariant(
            self.clients.get_mut(connection),
            "Pairing connection disappeared",
        )
    }

    /// The requests awaiting the person, as `connection` sees them: where the person's code is asked
    /// for, only the ones that came in on a window this connection opened.
    pub fn pending_pair_view(&self, connection: &str) -> Result<Value> {
        let rows: Vec<Pending> = StorePort::active_pair_requests(&self.store, self.now_unix())
            .map_err(errors::storage)?;
        let own_windows_only = self.approval().asks_for_code();
        let mut requests = vec![];
        for pending in rows {
            if pending.proved
                && pending.approved.is_none()
                && (!own_windows_only || self.owns_window_of(connection, &pending))
            {
                let public = auth::public_key(&pending.key)
                    .map_err(|error| errors::internal(error.to_string()))?;
                let expires_at = chrono::DateTime::from_timestamp(pending.expires, 0)
                    .ok_or_else(|| errors::internal("Pairing expiration is out of range"))?
                    .to_rfc3339();
                requests.push(DevicePairPending {
                    kind: "device_pair".into(),
                    request_id: pending.id,
                    device_name: pending.name,
                    fingerprint: auth::fingerprint(&public),
                    expires_at,
                });
            }
        }
        model::to_value(&requests)
    }

    fn pending_pair(&self, id: &str) -> Result<Pending> {
        active_pending(&self.store, self.clock.as_ref(), id)
    }

    pub fn pair_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        if self.pairing_client(connection)?.actor.is_some() {
            return Err(errors::unauthorized("Pairing requires a fresh connection"));
        }
        // Before any stored answer is looked up: a replay from where no device pairs gets none.
        self.refuse_local_pairing(connection)?;
        let fingerprint = auth::call_fingerprint(method, params);
        if method == method::PAIR_BEGIN {
            let begin: PairBeginParams = model::typed_params(params)?;
            return self.pair_begin(connection, &fingerprint, begin);
        }
        let prove: PairProveParams = model::typed_params(params)?;
        self.pair_prove(connection, &fingerprint, prove)
    }

    fn pair_begin(
        &mut self,
        connection: &str,
        fingerprint: &str,
        begin: PairBeginParams,
    ) -> Result<Value> {
        let PairBeginParams {
            code,
            device_name: name,
            device_public_key: key,
            idempotency_key: idem,
        } = begin;
        let public = auth::public_key(&key).map_err(|error| errors::invalid(error.to_string()))?;
        let scope = format!("pair:{}", auth::fingerprint(&public));
        let receipt =
            StorePort::idempotency_receipt(&self.store, &scope, &idem).map_err(errors::storage)?;
        if let Some(receipt) = receipt {
            if receipt.fingerprint != fingerprint {
                return Err(errors::idempotency_conflict(
                    "Pairing key reused with different content",
                ));
            }
            let result = receipt.result;
            let id = result["requestId"]
                .as_str()
                .ok_or_else(|| errors::internal("Missing pairing request"))?;
            self.pending_pair(id)?;
            self.pairing_client_mut(connection)?.pair_request = Some(id.into());
            return Ok(result);
        }
        if self.approval().asks_for_code() {
            check_device_name(&name)?;
            // A window nobody holds (its connection ended, or the engine restarted) has no one
            // to decide for it.
            if !self.pairing.windows.is_bound(&code) {
                return Err(errors::unauthorized("Pairing code expired or invalid"));
            }
        }
        let expires = StorePort::active_pair_window_expiry(&self.store, &code, self.now_unix())
            .map_err(errors::storage)?;
        let expires =
            expires.ok_or_else(|| errors::unauthorized("Pairing code expired or invalid"))?;
        let pending = Pending {
            id: Uuid::new_v4().to_string(),
            code,
            name,
            key,
            challenge: auth::random_bytes().map_err(|error| errors::internal(error.to_string()))?,
            proved: false,
            approved: None,
            token: None,
            device: None,
            expires,
        };
        let now = self.now_unix();
        // Signed here and kept in the receipt: a replay answers with the same proof.
        let engine_proof = auth::sign(
            &self.engine_key,
            &auth::engine_pair_bytes(&pending.id, &pending.challenge, public.as_bytes()),
        );
        let result = model::to_value(&PairBeginResult {
            request_id: pending.id.clone(),
            challenge: STANDARD.encode(pending.challenge),
            engine_proof: Some(engine_proof),
        })?;
        let mutation = StorePort::mutate(&mut self.store, &scope, &idem, fingerprint, |tx| {
            tx.insert_pair_request(&pending.id, &pending, expires, now)?;
            Ok((result, vec![]))
        })
        .map_err(errors::storage)?;
        self.pairing_client_mut(connection)?.pair_request =
            mutation.result["requestId"].as_str().map(str::to_owned);
        Ok(mutation.result)
    }

    fn pair_prove(
        &mut self,
        connection: &str,
        fingerprint: &str,
        prove: PairProveParams,
    ) -> Result<Value> {
        let PairProveParams {
            request_id: id,
            proof,
            idempotency_key: idem,
        } = prove;
        if self.pairing_client(connection)?.pair_request.as_deref() != Some(id.as_str()) {
            return Err(errors::unauthorized(
                "Begin pairing on this connection first",
            ));
        }
        let mut pending = self.pending_pair(&id)?;
        // A window nobody holds has no one to approve: no code is made for a device to show.
        if self.approval().asks_for_code() && !self.pairing.windows.is_bound(&pending.code) {
            return Err(errors::unauthorized("Pairing code expired or invalid"));
        }
        let public = auth::public_key(&pending.key)
            .map_err(|error| errors::unauthorized(error.to_string()))?;
        if !auth::pair_proof(
            &pending.code,
            &public,
            &pending.challenge,
            &pending.name,
            &proof,
        ) {
            return Err(errors::unauthorized("Invalid pairing proof"));
        }
        let scope = format!("pair:{}", auth::fingerprint(&public));
        let was_proved = pending.proved;
        pending.proved = true;
        let events = if was_proved {
            vec![]
        } else {
            vec![model::pending(&DevicePairRequestedDraft {
                request_id: id.clone(),
                device_name: pending.name.clone(),
                fingerprint: auth::fingerprint(&public),
            })?]
        };
        let result = model::to_value(&PairProveResult {
            status: "pending".into(),
            approval_code: None,
        })?;
        let mutation = StorePort::mutate(&mut self.store, &scope, &idem, fingerprint, |tx| {
            tx.update_pair_request(&id, &pending)?;
            Ok((result, events))
        })
        .map_err(errors::storage)?;
        self.publish(&mutation.events)?;
        if pending.approved.is_some() {
            self.send_pair_result(connection, &pending)?;
        }
        // Added after the stored result is made, as `device.pair.start` adds `reachability`: a
        // replay is given the code from memory and the store never holds it.
        let Some(approval_code) = self.approval_code_for(&pending)? else {
            return Ok(mutation.result);
        };
        let mut proved: PairProveResult = serde_json::from_value(mutation.result)
            .map_err(|_| errors::internal("Stored pairing result is malformed"))?;
        proved.approval_code = Some(approval_code);
        model::to_value(&proved)
    }

    /// Records what a tailnet lookup changed and tells live clients when reachability moved.
    /// Without a tailnet listener, pairing links fall back to the loopback address.
    pub fn apply_tailnet(&mut self, update: TailnetUpdate, loopback: &str) -> Result<()> {
        self.pairing.address = update
            .pair_address
            .map_or_else(|| loopback.to_string(), |address| address.to_string());
        if self.reachability == update.reachability {
            return Ok(());
        }
        self.reachability = update.reachability;
        self.transient_event(
            event::ENGINE_REACHABILITY_CHANGED,
            model::to_value(&self.reachability)?,
        )
    }

    fn send_pair_result(&self, connection: &str, pending: &Pending) -> Result<()> {
        let approved = pending.approved == Some(true);
        let result = PairResultEvent {
            request_id: pending.id.clone(),
            approved,
            device_token: approved.then(|| pending.token.clone()).flatten(),
            device_id: approved
                .then(|| pending.device.as_ref().map(|d| d.device_id.clone()))
                .flatten(),
            reason: (!approved).then(|| denial_text(self.approval()).into()),
        };
        if let Some(client) = self.clients.get(connection) {
            client.outbox.send(model::notification(
                event::PAIR_RESULT,
                model::to_value(&result)?,
            )?);
        }
        Ok(())
    }

    /// The check `dispatch` makes before it looks for a stored result: the answer to the app's call
    /// is kept under a replay scope that has no signed-app bit, so a shell that repeated the app's
    /// client name, parameters and key would be handed it (ADR 0163).
    pub(crate) fn authorize_before_replay(
        &self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<()> {
        match method {
            method::DEVICE_PAIR_START | method::DEVICE_PAIR_DECIDE | method::DEVICE_REVOKE => {
                self.authorize_device_management(connection, method, params)
            }
            _ => Ok(()),
        }
    }

    /// Who may change which devices can control this computer (ADR 0128, 0162, 0163, 0164). Opening a
    /// pairing window, deciding a request and revoking a device are the signed app's alone where the
    /// engine attests the app (macOS): a window lets a device put its own name in front of the
    /// person. Where it cannot (`Approval::Refused`), no connection is the app and all three are
    /// refused. Where the person's code stands in for the app (`Approval::PersonsCode`), the local
    /// human may call them, and deciding is the local connection that opened the request's
    /// window. A paired device may only revoke itself. `authorize_before_replay` asks before a
    /// stored result is looked up, and `device_request` asks again.
    fn authorize_device_management(
        &self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<()> {
        let client = self.clients.get(connection);
        let actor = client.and_then(|client| client.actor.as_ref());
        let own_revocation = method == method::DEVICE_REVOKE && {
            let target: DeviceRevokeParams = model::typed_params(params)?;
            actor.is_some_and(|actor| actor.device_id.as_deref() == Some(target.device_id.as_str()))
        };
        let approval = self.approval();
        let app = client.is_some_and(|client| approval.admits_local_human(client.signed_ui));
        let allowed = match actor.map(|actor| &actor.role) {
            Some(Role::HumanLocal) => app,
            Some(Role::HumanDevice) => own_revocation,
            _ => false,
        };
        if allowed {
            if approval.asks_for_code() && method == method::DEVICE_PAIR_DECIDE {
                self.require_own_window(connection, params)?;
            }
            return Ok(());
        }
        Err(refusal(
            matches!(actor.map(|actor| &actor.role), Some(Role::HumanLocal)),
            approval,
        ))
    }

    /// A request is decided only by the connection that opened its window (ADR 0164). A request
    /// that is gone is not this check's to answer: the call finds that out itself, or replays its
    /// stored result.
    fn require_own_window(&self, connection: &str, params: &Value) -> Result<()> {
        let decide: DevicePairDecideParams = model::typed_params(params)?;
        let pending: Option<Pending> =
            StorePort::active_pair_request(&self.store, &decide.request_id, self.now_unix())
                .map_err(errors::storage)?;
        if pending.is_none_or(|pending| self.owns_window_of(connection, &pending)) {
            return Ok(());
        }
        Err(errors::forbidden_role(
            "A pairing request is decided in the window that opened it",
        ))
    }

    pub fn device_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        self.authorize_device_management(connection, method, params)?;
        match method {
            method::DEVICE_PAIR_START => self.device_pair_start(connection, method, params),
            method::DEVICE_REVOKE => self.device_revoke(connection, method, params),
            method::DEVICE_PAIR_DECIDE => self.device_pair_decide(connection, method, params),
            _ => Err(errors::internal("Method is not a device management method")),
        }
    }

    fn device_pair_start(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let code = auth::token().map_err(|error| errors::internal(error.to_string()))?;
        let now = self.now_unix();
        let expires = now + limits::PAIRING_WINDOW_SECONDS as i64;
        let expires_at = chrono::DateTime::from_timestamp(expires, 0)
            .ok_or_else(|| errors::internal("Pairing expiration is out of range"))?;
        let key = auth::engine_key_text(&self.engine_key);
        let uri = pairing_link::uri(self.store.store_id(), &self.pairing.address, &code, &key);
        let qr = pairing_link::qr(self.store.store_id(), &self.pairing.address, &code, &key);
        let started = model::to_value(&DevicePairStartResult {
            code: code.clone(),
            expires_at: expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            uri,
            qr: Some(qr),
            reachability: None,
        })?;
        let result = self
            .mutate(connection, method, params, |tx| {
                tx.open_pair_window(&code, expires, now)?;
                Ok((started, vec![]))
            })?
            .result;
        // Live state, not part of the idempotent record: a replay sees today's reachability.
        let mut result: DevicePairStartResult = serde_json::from_value(result)
            .map_err(|_| errors::internal("Stored pairing result is malformed"))?;
        // Only a window made now is this connection's; a replayed one stays with its opener.
        if self.approval().asks_for_code() && result.code == code {
            self.pairing.windows.bind(&code, connection);
        }
        result.reachability = Some(self.reachability.clone());
        let result = model::to_value(&result)?;
        Ok(result)
    }

    fn device_revoke(&mut self, connection: &str, method: &str, params: &Value) -> Result<Value> {
        let revoke: DeviceRevokeParams = model::typed_params(params)?;
        let id = revoke.device_id.as_str();
        let mut device: Device = StorePort::device(&self.store, id)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::not_found("Device not found"))?;
        device.revoked = true;
        let revoked = model::to_value(&DeviceRevokeResult {
            device_id: revoke.device_id.clone(),
        })?;
        let event = model::pending(&DeviceRevokedDraft {
            device_id: revoke.device_id.clone(),
        })?;
        let result = self
            .mutate(connection, method, params, |tx| {
                tx.revoke_device(id, &device)?;
                Ok((revoked, vec![event]))
            })?
            .result;
        for client in self
            .clients
            .values()
            .filter(|c| c.actor.as_ref().and_then(|a| a.device_id.as_deref()) == Some(id))
        {
            client
                .outbox
                .close(crate::outbox::CloseReason::DeviceRevoked);
        }
        Ok(result)
    }

    fn device_pair_decide(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let decide: DevicePairDecideParams = model::typed_params(params)?;
        let id = decide.request_id.as_str();
        let mut pending = self.pending_pair(id)?;
        if !pending.proved || pending.approved.is_some() {
            return Err(errors::transition_refused(
                "Pairing request is not awaiting consent",
            ));
        }
        let public =
            auth::public_key(&pending.key).map_err(|error| errors::invalid(error.to_string()))?;
        let approve = decide.approve;
        let asks_for_code = self.approval().asks_for_code();
        if asks_for_code && !self.owns_window_of(connection, &pending) {
            return Err(errors::forbidden_role(
                "A pairing request is decided in the window that opened it",
            ));
        }
        let held = pending.clone();
        pending.approved = Some(approve);
        if approve {
            let available =
                StorePort::pair_window_active(&self.store, &pending.code, self.now_unix())
                    .map_err(errors::storage)?;
            if !available {
                return Err(errors::unauthorized("Pairing code expired or already used"));
            }
            if asks_for_code {
                self.check_approval_code(&held, decide.approval_code.as_deref())?;
            }
            pending.token =
                Some(auth::token().map_err(|error| errors::internal(error.to_string()))?);
            pending.device = Some(Device {
                device_id: Uuid::new_v4().to_string(),
                name: pending.name.clone(),
                fingerprint: auth::fingerprint(&public),
                revoked: false,
            });
        }
        let decided = model::to_value(&DevicePairDecideResult {
            request_id: decide.request_id.clone(),
            approved: approve,
        })?;
        let paired = pending
            .device
            .as_ref()
            .map(|device| {
                model::pending(&DevicePairedDraft {
                    device: device.clone(),
                })
            })
            .transpose()?;
        let result = self
            .mutate(connection, method, params, |tx| {
                let mut events = vec![];
                if let Some(device) = &pending.device {
                    let token = pending
                        .token
                        .as_deref()
                        .ok_or(soyeht_store::database::Error::ApprovedPairTokenMissing)?;
                    tx.consume_pair_window_and_insert_device(
                        &pending.code,
                        &device.device_id,
                        device,
                        public.as_bytes(),
                        &auth::token_hash(token),
                    )?;
                    events.extend(paired);
                }
                tx.update_pair_request(id, &pending)?;
                Ok((decided, events))
            })?
            .result;
        self.pairing.codes.forget(id);
        for (connection, _) in self
            .clients
            .iter()
            .filter(|(_, c)| c.pair_request.as_deref() == Some(id))
        {
            self.send_pair_result(connection, &pending)?;
        }
        Ok(result)
    }

    /// The third wrong code denies the request, as the person's own Deny would: it leaves the
    /// pending list of every window, and the device is told with `pair.result`. The window that
    /// opened the request is told the same way, since the call that made the third wrong code
    /// is answered with a refusal, not with the denial a Deny gets as its result.
    pub(crate) fn deny_after_wrong_codes(&mut self, pending: &Pending) -> Result<()> {
        let mut denied = pending.clone();
        denied.approved = Some(false);
        let scope = format!("pair-wrong-codes:{}", pending.id);
        StorePort::mutate(&mut self.store, &scope, "denied", "denied", |tx| {
            tx.update_pair_request(&denied.id, &denied)?;
            Ok((json!({}), vec![]))
        })
        .map_err(errors::storage)?;
        for (connection, _) in self
            .clients
            .iter()
            .filter(|(_, c)| c.pair_request.as_deref() == Some(denied.id.as_str()))
        {
            self.send_pair_result(connection, &denied)?;
        }
        if let Some(window) = self.pairing.windows.owner(&denied.code) {
            self.send_pair_result(window, &denied)?;
        }
        Ok(())
    }

    /// The rule in force: a test's choice, else the system's.
    pub(crate) fn approval(&self) -> Approval {
        self.hooks
            .pairing_approval()
            .unwrap_or_else(Approval::for_this_system)
    }

    /// `pair.begin` and `pair.prove` come from another device, never from a program on this
    /// computer's own endpoint, which would be the person's request and its own approval at once.
    pub(crate) fn refuse_local_pairing(&self, connection: &str) -> Result<()> {
        let local = self
            .clients
            .get(connection)
            .is_some_and(|client| client.local);
        if self.approval().asks_for_code() && local {
            return Err(errors::unauthorized(
                "A device pairs from another device, not from this computer",
            ));
        }
        Ok(())
    }

    /// The code `pair.prove` answers a device with, when this engine asks for one and the request
    /// still awaits a decision.
    pub(crate) fn approval_code_for(&mut self, pending: &Pending) -> Result<Option<String>> {
        if !self.approval().asks_for_code() || pending.approved.is_some() {
            return Ok(None);
        }
        let now = self.now_unix();
        self.pairing
            .codes
            .issue(&pending.id, pending.expires, now)
            .map(Some)
    }

    /// Whether `connection` is the one whose window `pending` came in on.
    pub(crate) fn owns_window_of(&self, connection: &str, pending: &Pending) -> bool {
        self.pairing.windows.is_bound_to(&pending.code, connection)
    }

    /// Checks the code the person typed for `pending`. A wrong code answers
    /// `pairing_code_invalid` and approves nothing; the last wrong one also denies the request.
    pub(crate) fn check_approval_code(
        &mut self,
        pending: &Pending,
        typed: Option<&str>,
    ) -> Result<()> {
        let now = self.now_unix();
        match self.pairing.codes.check(&pending.id, typed, now) {
            Verdict::Right => Ok(()),
            Verdict::Missing => Err(errors::pairing_code_invalid(
                "Type the code your phone shows",
            )),
            Verdict::Malformed => Err(errors::pairing_code_invalid(
                "Type the six digits your phone shows",
            )),
            Verdict::NoCode => Err(errors::pairing_code_invalid(
                "No code is waiting for this request; pair again on the phone",
            )),
            Verdict::Wrong { left } => Err(errors::pairing_code_invalid(format!(
                "That code is not right. {left} tries left"
            ))),
            Verdict::Exhausted => {
                self.deny_after_wrong_codes(pending)?;
                Err(errors::pairing_code_invalid(
                    "Too many wrong codes. The request was denied",
                ))
            }
        }
    }
}

#[cfg(test)]
mod code_tests;
#[cfg(test)]
mod tests;
