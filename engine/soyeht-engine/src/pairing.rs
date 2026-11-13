use crate::{
    auth,
    core::Core,
    errors::{self, Result},
    model,
    network::TailnetUpdate,
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

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Pending {
    id: String,
    code: String,
    name: String,
    key: String,
    challenge: [u8; 32],
    proved: bool,
    approved: Option<bool>,
    token: Option<String>,
    device: Option<Device>,
    expires: i64,
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
fn refusal(local_human: bool) -> errors::Error {
    static LOGGED: Once = Once::new();
    // No signature to blame and nothing to log: this is the system's rule, not an engine fault.
    if local_human && !ui_attestation::available() {
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

    pub fn pending_pair_view(&self) -> Result<Value> {
        let rows: Vec<Pending> = StorePort::active_pair_requests(&self.store, self.now_unix())
            .map_err(errors::storage)?;
        let mut requests = vec![];
        for pending in rows {
            if pending.proved && pending.approved.is_none() {
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
        let fingerprint = STANDARD.encode(auth::token_hash(&json!([method, params]).to_string()));
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
        Ok(mutation.result)
    }

    /// Records what a tailnet lookup changed and tells live clients when reachability moved.
    /// Without a tailnet listener, pairing links fall back to the loopback address.
    pub fn apply_tailnet(&mut self, update: TailnetUpdate, loopback: &str) -> Result<()> {
        self.pair_address = update
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
            reason: (!approved).then(|| "Denied on Mac".into()),
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

    /// Who may change which devices can control this computer (ADR 0128, 0162, 0163). Opening a
    /// pairing window, deciding a request and revoking a device are the signed app's alone, on every
    /// system: a window lets a device put its own name in front of the person. Where the engine
    /// cannot attest the app (`ui_attestation::available`), no connection is the app and all three
    /// are refused. A paired device may only revoke itself. `authorize_before_replay` asks before a
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
        let app = client.is_some_and(|client| client.signed_ui);
        let allowed = match actor.map(|actor| &actor.role) {
            Some(Role::HumanLocal) => app,
            Some(Role::HumanDevice) => own_revocation,
            _ => false,
        };
        if allowed {
            return Ok(());
        }
        Err(refusal(matches!(
            actor.map(|actor| &actor.role),
            Some(Role::HumanLocal)
        )))
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
        let uri = pairing_link::uri(self.store.store_id(), &self.pair_address, &code, &key);
        let qr = pairing_link::qr(self.store.store_id(), &self.pair_address, &code, &key);
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
        pending.approved = Some(approve);
        if approve {
            let available =
                StorePort::pair_window_active(&self.store, &pending.code, self.now_unix())
                    .map_err(errors::storage)?;
            if !available {
                return Err(errors::unauthorized("Pairing code expired or already used"));
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
        for (connection, _) in self
            .clients
            .iter()
            .filter(|(_, c)| c.pair_request.as_deref() == Some(id))
        {
            self.send_pair_result(connection, &pending)?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::testing::{FixedClock, MemoryStore};
    use soyeht_protocol::ErrorKind;

    fn pending(expires: i64) -> Pending {
        Pending {
            id: "request".into(),
            code: "123456".into(),
            name: "Phone".into(),
            key: "key".into(),
            challenge: [7; 32],
            proved: false,
            approved: None,
            token: None,
            device: None,
            expires,
        }
    }

    #[test]
    fn pairing_rule_uses_the_clock_and_store_ports_for_expiry() {
        let mut store = MemoryStore::default();
        store
            .pair_requests
            .insert("request".into(), serde_json::to_value(pending(11)).unwrap());

        assert_eq!(
            active_pending(&store, &FixedClock(10_000), "request")
                .unwrap()
                .name,
            "Phone"
        );
        let error = active_pending(&store, &FixedClock(11_000), "request").unwrap_err();
        assert_eq!(errors::kind(&error), ErrorKind::Unauthorized);
    }

    #[tokio::test]
    async fn pairing_is_decided_by_the_mac_app_and_never_by_a_paired_device() {
        use crate::outbox;
        use soyeht_protocol::Actor;
        let directory =
            std::env::temp_dir().join(format!("soyeht-pairing-roles-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        let (local_outbox, _local) = outbox::channel();
        // A shell or script on the local socket, and the signed app (a peer the system attests).
        let local = core.connect(true, false, local_outbox).unwrap();
        let (app_outbox, _app) = outbox::channel();
        let app = core.connect(true, true, app_outbox).unwrap();
        for connection in [&local, &app] {
            let hello = json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{
                "client":"roles-test","clientVersion":"1","protocolVersion":1}});
            core.dispatch(connection, hello).unwrap();
        }
        let (device_outbox, _device) = outbox::channel();
        let phone = core.connect(false, false, device_outbox).unwrap();
        core.clients.get_mut(&phone).unwrap().actor = Some(Actor {
            role: Role::HumanDevice,
            client: "phone".into(),
            device_id: Some("device-1".into()),
            pane_id: None,
        });
        // The error kind of each call, or `None` when it went through.
        let mut kinds = |connection: &str| {
            let calls = [
                ("device.pair.start", json!({})),
                (
                    "device.pair.decide",
                    json!({"requestId":"r","approve":true}),
                ),
                ("device.revoke", json!({"deviceId":"device-2"})),
                ("device.revoke", json!({"deviceId":"device-1"})),
            ];
            calls.map(|(method, mut params)| {
                params["idempotencyKey"] = json!(Uuid::new_v4().to_string());
                let request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
                core.dispatch(connection, request)
                    .err()
                    .map(|error| errors::kind(&error))
            })
        };

        // A device is refused all but its own revocation, which gets as far as the lookup.
        assert_eq!(
            kinds(&phone),
            [
                Some(ErrorKind::ForbiddenRole),
                Some(ErrorKind::ForbiddenRole),
                Some(ErrorKind::ForbiddenRole),
                Some(ErrorKind::NotFound)
            ]
        );
        // The app keeps all of them: the window opens, the rest find nothing to act on.
        let app_kinds = || {
            [
                None,
                Some(ErrorKind::Unauthorized),
                Some(ErrorKind::NotFound),
                Some(ErrorKind::NotFound),
            ]
        };
        assert_eq!(kinds(&app), app_kinds());
        // A shell cannot open a window, decide a request or revoke a device, on any system.
        let refused = || Some(ErrorKind::ForbiddenRole);
        assert_eq!(kinds(&local), [refused(), refused(), refused(), refused()]);
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// An engine that is not signed refuses even the app, and says why.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn an_unsigned_engine_says_why_it_cannot_tell_the_app_from_a_shell() {
        let (client, _server) = tokio::net::UnixStream::pair().unwrap();
        let token = ui_attestation::peer_audit_token(&client).unwrap();
        assert!(!ui_attestation::signed_ui_peer(
            token,
            &soyeht_protocol::profile::DEV
        ));
        assert!(ui_attestation::engine_cannot_attest());
        assert!(
            refusal(true)
                .to_string()
                .contains("does not carry the Soyeht signature")
        );
        assert!(
            !refusal(false)
                .to_string()
                .contains("does not carry the Soyeht signature")
        );
    }

    /// Where nothing is attested, the refusal says the app cannot yet prove itself, not that the
    /// person should use an app that has no way to be believed.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_system_without_attestation_says_the_app_cannot_yet_prove_itself() {
        assert!(!ui_attestation::available());
        let message = refusal(true).to_string();
        assert!(message.contains("cannot yet prove itself"), "{message}");
        assert!(message.contains("Soyeht app"), "{message}");
    }

    #[tokio::test]
    async fn tailnet_reachability_reaches_status_pairing_and_live_clients() {
        use crate::{dispatch::Outcome, network, outbox};
        use soyeht_protocol::{EngineReachabilityReason, frame::Frame};
        let directory =
            std::env::temp_dir().join(format!("soyeht-reachability-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        let (outbox, mut receiver) = outbox::channel();
        let connection = core.connect(true, true, outbox).unwrap();
        let _challenge = receiver.next().await.unwrap().unwrap();
        let hello = json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{
            "client":"reachability-test","clientVersion":"1","protocolVersion":1}});
        core.dispatch(&connection, hello).unwrap();
        core.clients.get_mut(&connection).unwrap().events = true;
        let call = |core: &mut Core, id: u32, method: &str| {
            let params = json!({"idempotencyKey":Uuid::new_v4().to_string()});
            let request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
            match core.dispatch(&connection, request).unwrap() {
                Outcome::Reply(result) => result,
                _ => panic!("{method} did not reply"),
            }
        };
        let loopback = core.pair_address.clone();

        let missing = network::unreachable(EngineReachabilityReason::TailnetAddressNotFound);
        let update = |reachability, pair_address| TailnetUpdate {
            reachability,
            pair_address,
        };
        core.apply_tailnet(update(missing.clone(), None), &loopback)
            .unwrap();
        let status = call(&mut core, 2, "engine.status");
        assert_eq!(
            status["reachability"],
            json!({"remote":false,"reason":"tailnet_address_not_found"})
        );
        let pair = call(&mut core, 3, "device.pair.start");
        assert_eq!(pair["reachability"]["remote"], false);
        assert!(pair["uri"].as_str().unwrap().contains(&loopback));

        let tailnet: std::net::SocketAddr = "100.64.0.9:7777".parse().unwrap();
        core.apply_tailnet(update(network::reachable(), Some(tailnet)), &loopback)
            .unwrap();
        let mut announced = vec![];
        while let Ok(Some(packet)) =
            tokio::time::timeout(std::time::Duration::from_millis(50), receiver.next())
                .await
                .unwrap_or(Ok(None))
        {
            if let Frame::Json(bytes) = packet.frame {
                let message: Value = serde_json::from_slice(&bytes).unwrap();
                if message["method"] == "engine.reachability_changed" {
                    announced.push(message["params"].clone());
                }
            }
        }
        assert_eq!(
            announced,
            [
                json!({"remote":false,"reason":"tailnet_address_not_found"}),
                json!({"remote":true})
            ]
        );
        assert_eq!(
            call(&mut core, 4, "engine.status")["reachability"]["remote"],
            true
        );
        let pair = call(&mut core, 5, "device.pair.start");
        assert!(
            pair["uri"]
                .as_str()
                .unwrap()
                .contains("address=100.64.0.9:7777")
        );
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
