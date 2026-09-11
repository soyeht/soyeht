use crate::{
    auth,
    core::Core,
    errors::{self, Result},
    model,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::{Device, ErrorKind};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
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

impl Core {
    pub fn pending_pair_view(&self) -> Result<Value> {
        let rows: Vec<Pending> = self
            .store
            .active_pair_requests(Utc::now().timestamp())
            .map_err(errors::storage)?;
        let mut requests = vec![];
        for pending in rows {
            if pending.proved && pending.approved.is_none() {
                let public = auth::public_key(&pending.key)
                    .map_err(|s| errors::error(ErrorKind::InternalError, s))?;
                requests.push(json!({"kind":"device_pair","requestId":pending.id,"deviceName":pending.name,"fingerprint":auth::fingerprint(&public),"expiresAt":chrono::DateTime::from_timestamp(pending.expires,0).expect("timestamp").to_rfc3339()}));
            }
        }
        Ok(json!(requests))
    }

    fn pending_pair(&self, id: &str) -> Result<Pending> {
        self.store
            .active_pair_request(id, Utc::now().timestamp())
            .map_err(errors::storage)?
            .ok_or_else(|| {
                errors::error(
                    ErrorKind::Unauthorized,
                    "Pairing request missing or expired",
                )
            })
    }

    pub fn pair_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        if self.clients[connection].actor.is_some() {
            return Err(errors::error(
                ErrorKind::Unauthorized,
                "Pairing requires a fresh connection",
            ));
        }
        let fingerprint = STANDARD.encode(auth::token_hash(&json!([method, params]).to_string()));
        let idem = params["idempotencyKey"].as_str().expect("key");
        if method == "pair.begin" {
            let code = params["code"].as_str().expect("code");
            let key = params["devicePublicKey"].as_str().expect("key");
            let public =
                auth::public_key(key).map_err(|s| errors::error(ErrorKind::InvalidParams, s))?;
            let scope = format!("pair:{}", auth::fingerprint(&public));
            let receipt = self
                .store
                .idempotency_receipt(&scope, idem)
                .map_err(errors::storage)?;
            if let Some(receipt) = receipt {
                if receipt.fingerprint != fingerprint {
                    return Err(errors::error(
                        ErrorKind::IdempotencyConflict,
                        "Pairing key reused with different content",
                    ));
                }
                let result = receipt.result;
                let id = result["requestId"].as_str().ok_or_else(|| {
                    errors::error(ErrorKind::InternalError, "Missing pairing request")
                })?;
                self.pending_pair(id)?;
                self.clients
                    .get_mut(connection)
                    .expect("connection")
                    .pair_request = Some(id.into());
                return Ok(result);
            }
            let expires = self
                .store
                .active_pair_window_expiry(code, Utc::now().timestamp())
                .map_err(errors::storage)?;
            let expires = expires.ok_or_else(|| {
                errors::error(ErrorKind::Unauthorized, "Pairing code expired or invalid")
            })?;
            let pending = Pending {
                id: Uuid::new_v4().to_string(),
                code: code.into(),
                name: params["deviceName"].as_str().expect("name").into(),
                key: key.into(),
                challenge: auth::random_bytes()
                    .map_err(|s| errors::error(ErrorKind::InternalError, s))?,
                proved: false,
                approved: None,
                token: None,
                device: None,
                expires,
            };
            let mutation=self.store.mutate(&scope,idem,&fingerprint,|tx| {
                tx.insert_pair_request(&pending.id, &pending, expires, Utc::now().timestamp())?;
                Ok((json!({"requestId":pending.id,"challenge":STANDARD.encode(pending.challenge)}),vec![]))
            }).map_err(errors::storage)?;
            self.clients
                .get_mut(connection)
                .expect("connection")
                .pair_request = mutation.result["requestId"].as_str().map(str::to_owned);
            return Ok(mutation.result);
        }
        let id = params["requestId"].as_str().expect("request");
        if self.clients[connection].pair_request.as_deref() != Some(id) {
            return Err(errors::error(
                ErrorKind::Unauthorized,
                "Begin pairing on this connection first",
            ));
        }
        let mut pending = self.pending_pair(id)?;
        let public = auth::public_key(&pending.key)
            .map_err(|s| errors::error(ErrorKind::Unauthorized, s))?;
        if !auth::pair_proof(
            &pending.code,
            &public,
            &pending.challenge,
            &pending.name,
            params["proof"].as_str().expect("proof"),
        ) {
            return Err(errors::error(
                ErrorKind::Unauthorized,
                "Invalid pairing proof",
            ));
        }
        let scope = format!("pair:{}", auth::fingerprint(&public));
        let was_proved = pending.proved;
        pending.proved = true;
        let mutation=self.store.mutate(&scope,idem,&fingerprint,|tx| {
            tx.update_pair_request(id, &pending)?;
            let events=if was_proved {vec![]}else{vec![("device.pair_requested".into(),json!({"requestId":id,"deviceName":pending.name,"fingerprint":auth::fingerprint(&public)}))]};
            Ok((json!({"status":"pending"}),events))
        }).map_err(errors::storage)?;
        self.publish(&mutation.events);
        if pending.approved.is_some() {
            self.send_pair_result(connection, &pending);
        }
        Ok(mutation.result)
    }

    fn send_pair_result(&self, connection: &str, pending: &Pending) {
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
                .send(model::notification("pair.result", result));
        }
    }

    pub fn device_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        if method == "device.pair.start" {
            let code = auth::token().map_err(|s| errors::error(ErrorKind::InternalError, s))?;
            let expires = Utc::now() + chrono::Duration::minutes(10);
            let uri = format!(
                "soyeht-next://pair?engineId={}&address={}&code={}",
                self.store.db_id, self.pair_address, code
            );
            return Ok(self.mutate(connection,method,params,|tx| {
                tx.open_pair_window(&code, expires.timestamp(), Utc::now().timestamp())?;
                Ok((json!({"code":code,"expiresAt":expires.to_rfc3339_opts(SecondsFormat::Secs,true),"uri":uri}),vec![]))
            })?.result);
        }
        if method == "device.revoke" {
            let id = params["deviceId"].as_str().expect("device");
            let mut device: Device = self
                .store
                .device(id)
                .map_err(errors::storage)?
                .ok_or_else(|| errors::error(ErrorKind::NotFound, "Device not found"))?;
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
        let id = params["requestId"].as_str().expect("request");
        let mut pending = self.pending_pair(id)?;
        if !pending.proved || pending.approved.is_some() {
            return Err(errors::error(
                ErrorKind::TransitionRefused,
                "Pairing request is not awaiting consent",
            ));
        }
        let public = auth::public_key(&pending.key)
            .map_err(|s| errors::error(ErrorKind::InvalidParams, s))?;
        let approve = params["approve"].as_bool().expect("approve");
        pending.approved = Some(approve);
        if approve {
            let available = self
                .store
                .pair_window_active(&pending.code, Utc::now().timestamp())
                .map_err(errors::storage)?;
            if !available {
                return Err(errors::error(
                    ErrorKind::Unauthorized,
                    "Pairing code expired or already used",
                ));
            }
            pending.token =
                Some(auth::token().map_err(|s| errors::error(ErrorKind::InternalError, s))?);
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
                    let token =
                        pending
                            .token
                            .as_deref()
                            .ok_or(soyeht_store::database::Error::Invalid(
                                "approved pair request has no token",
                            ))?;
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
            self.send_pair_result(connection, &pending);
        }
        Ok(result)
    }
}
