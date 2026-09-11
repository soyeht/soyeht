use crate::{
    auth,
    core::Core,
    errors::{self, Result},
    model,
    ports::{Clock, StorePort},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::Device;
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
                requests.push(json!({"kind":"device_pair","requestId":pending.id,"deviceName":pending.name,"fingerprint":auth::fingerprint(&public),"expiresAt":expires_at}));
            }
        }
        Ok(json!(requests))
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
        let idem = errors::param_str(params, "idempotencyKey")?;
        if method == "pair.begin" {
            let code = errors::param_str(params, "code")?;
            let key = errors::param_str(params, "devicePublicKey")?;
            let name = errors::param_str(params, "deviceName")?;
            let public =
                auth::public_key(key).map_err(|error| errors::invalid(error.to_string()))?;
            let scope = format!("pair:{}", auth::fingerprint(&public));
            let receipt = StorePort::idempotency_receipt(&self.store, &scope, idem)
                .map_err(errors::storage)?;
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
            let expires = StorePort::active_pair_window_expiry(&self.store, code, self.now_unix())
                .map_err(errors::storage)?;
            let expires =
                expires.ok_or_else(|| errors::unauthorized("Pairing code expired or invalid"))?;
            let pending = Pending {
                id: Uuid::new_v4().to_string(),
                code: code.into(),
                name: name.into(),
                key: key.into(),
                challenge: auth::random_bytes()
                    .map_err(|error| errors::internal(error.to_string()))?,
                proved: false,
                approved: None,
                token: None,
                device: None,
                expires,
            };
            let now = self.now_unix();
            let mutation = StorePort::mutate(&mut self.store, &scope, idem, &fingerprint, |tx| {
                tx.insert_pair_request(&pending.id, &pending, expires, now)?;
                Ok((
                    json!({"requestId":pending.id,"challenge":STANDARD.encode(pending.challenge)}),
                    vec![],
                ))
            })
            .map_err(errors::storage)?;
            self.pairing_client_mut(connection)?.pair_request =
                mutation.result["requestId"].as_str().map(str::to_owned);
            return Ok(mutation.result);
        }
        let id = errors::param_str(params, "requestId")?;
        if self.pairing_client(connection)?.pair_request.as_deref() != Some(id) {
            return Err(errors::unauthorized(
                "Begin pairing on this connection first",
            ));
        }
        let mut pending = self.pending_pair(id)?;
        let public = auth::public_key(&pending.key)
            .map_err(|error| errors::unauthorized(error.to_string()))?;
        if !auth::pair_proof(
            &pending.code,
            &public,
            &pending.challenge,
            &pending.name,
            errors::param_str(params, "proof")?,
        ) {
            return Err(errors::unauthorized("Invalid pairing proof"));
        }
        let scope = format!("pair:{}", auth::fingerprint(&public));
        let was_proved = pending.proved;
        pending.proved = true;
        let mutation=StorePort::mutate(&mut self.store,&scope,idem,&fingerprint,|tx| {
            tx.update_pair_request(id, &pending)?;
            let events=if was_proved {vec![]}else{vec![("device.pair_requested".into(),json!({"requestId":id,"deviceName":pending.name,"fingerprint":auth::fingerprint(&public)}))]};
            Ok((json!({"status":"pending"}),events))
        }).map_err(errors::storage)?;
        self.publish(&mutation.events)?;
        if pending.approved.is_some() {
            self.send_pair_result(connection, &pending)?;
        }
        Ok(mutation.result)
    }

    fn send_pair_result(&self, connection: &str, pending: &Pending) -> Result<()> {
        let approved = pending.approved == Some(true);
        let mut result = json!({"requestId":pending.id,"approved":approved});
        if approved {
            result["deviceToken"] = json!(pending.token);
            result["deviceId"] = json!(pending.device.as_ref().map(|d| &d.device_id));
        } else {
            result["reason"] = json!("Denied on Mac");
        }
        if let Some(client) = self.clients.get(connection) {
            client
                .outbox
                .send(model::notification("pair.result", result)?);
        }
        Ok(())
    }

    pub fn device_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        if method == "device.pair.start" {
            let code = auth::token().map_err(|error| errors::internal(error.to_string()))?;
            let now = self.now_unix();
            let expires = now + chrono::Duration::minutes(10).num_seconds();
            let expires_at = chrono::DateTime::from_timestamp(expires, 0)
                .ok_or_else(|| errors::internal("Pairing expiration is out of range"))?;
            let uri = format!(
                "soyeht-next://pair?engineId={}&address={}&code={}",
                self.store.store_id(),
                self.pair_address,
                code
            );
            return Ok(self.mutate(connection,method,params,|tx| {
                tx.open_pair_window(&code, expires, now)?;
                Ok((json!({"code":code,"expiresAt":expires_at.to_rfc3339_opts(SecondsFormat::Secs,true),"uri":uri}),vec![]))
            })?.result);
        }
        if method == "device.revoke" {
            let id = errors::param_str(params, "deviceId")?;
            let mut device: Device = StorePort::device(&self.store, id)
                .map_err(errors::storage)?
                .ok_or_else(|| errors::not_found("Device not found"))?;
            device.revoked = true;
            let result = self
                .mutate(connection, method, params, |tx| {
                    tx.revoke_device(id, &device)?;
                    Ok((
                        json!({"deviceId":id}),
                        vec![("device.revoked".into(), json!({"deviceId":id}))],
                    ))
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
            return Ok(result);
        }
        let id = errors::param_str(params, "requestId")?;
        let mut pending = self.pending_pair(id)?;
        if !pending.proved || pending.approved.is_some() {
            return Err(errors::transition_refused(
                "Pairing request is not awaiting consent",
            ));
        }
        let public =
            auth::public_key(&pending.key).map_err(|error| errors::invalid(error.to_string()))?;
        let approve = errors::param_bool(params, "approve")?;
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
                    events.push(("device.paired".into(), json!({"device":device})));
                }
                tx.update_pair_request(id, &pending)?;
                Ok((json!({"requestId":id,"approved":approve}), events))
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
}
