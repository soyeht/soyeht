use crate::{
    auth,
    core::Core,
    errors::{self, Result},
    model,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
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
        let mut statement = self
            .store
            .connection()
            .prepare("SELECT payload FROM pair_requests WHERE expires_unix>?1 ORDER BY id")
            .map_err(|e| errors::storage(e.into()))?;
        let rows = statement
            .query_map([Utc::now().timestamp()], |r| r.get::<_, String>(0))
            .map_err(|e| errors::storage(e.into()))?;
        let mut requests = vec![];
        for row in rows {
            let pending: Pending = serde_json::from_str(
                &row.map_err(|e| errors::storage(e.into()))?,
            )
            .map_err(|_| errors::error(ErrorKind::InternalError, "Invalid pairing record"))?;
            if pending.proved && pending.approved.is_none() {
                let public = auth::public_key(&pending.key)
                    .map_err(|s| errors::error(ErrorKind::InternalError, s))?;
                requests.push(json!({"kind":"device_pair","requestId":pending.id,"deviceName":pending.name,"fingerprint":auth::fingerprint(&public),"expiresAt":chrono::DateTime::from_timestamp(pending.expires,0).expect("timestamp").to_rfc3339()}));
            }
        }
        Ok(json!(requests))
    }

    fn pending_pair(&self, id: &str) -> Result<Pending> {
        let raw: Option<String> = self
            .store
            .connection()
            .query_row(
                "SELECT payload FROM pair_requests WHERE id=?1 AND expires_unix>?2",
                params![id, Utc::now().timestamp()],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| errors::storage(e.into()))?;
        serde_json::from_str(&raw.ok_or_else(|| {
            errors::error(
                ErrorKind::Unauthorized,
                "Pairing request missing or expired",
            )
        })?)
        .map_err(|_| errors::error(ErrorKind::InternalError, "Invalid pairing record"))
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
            let receipt: Option<(String, String)> = self
                .store
                .connection()
                .query_row(
                    "SELECT fingerprint,result FROM idempotency WHERE scope=?1 AND key=?2",
                    params![scope, idem],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| errors::storage(e.into()))?;
            if let Some((previous, result)) = receipt {
                if previous != fingerprint {
                    return Err(errors::error(
                        ErrorKind::IdempotencyConflict,
                        "Pairing key reused with different content",
                    ));
                }
                let result: Value = serde_json::from_str(&result).map_err(|_| {
                    errors::error(ErrorKind::InternalError, "Invalid pairing receipt")
                })?;
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
            let expires: Option<i64> = self
                .store
                .connection()
                .query_row(
                    "SELECT expires_unix FROM pair_windows WHERE code=?1 AND expires_unix>?2",
                    params![code, Utc::now().timestamp()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| errors::storage(e.into()))?;
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
                let count:i64=tx.query_row("SELECT COUNT(*) FROM pair_requests WHERE expires_unix>?1",[Utc::now().timestamp()],|r|r.get(0))?;
                if count>=64 {return Err(soyeht_store::database::Error::Invalid("pairing request limit reached"));}
                tx.execute("INSERT INTO pair_requests(id,payload,expires_unix) VALUES (?1,?2,?3)",params![pending.id,serde_json::to_string(&pending)?,expires])?;
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
            tx.execute("UPDATE pair_requests SET payload=?1 WHERE id=?2",params![serde_json::to_string(&pending)?,id])?;
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
            result["reason"] = json!("Recusado no Mac");
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
                tx.execute("DELETE FROM pair_windows WHERE expires_unix<=?1",[Utc::now().timestamp()])?;
                tx.execute("DELETE FROM pair_requests WHERE expires_unix<=?1",[Utc::now().timestamp()])?;
                tx.execute("INSERT INTO pair_windows(code,expires_unix) VALUES (?1,?2)",params![code,expires.timestamp()])?;
                Ok((json!({"code":code,"expiresAt":expires.to_rfc3339_opts(SecondsFormat::Secs,true),"uri":uri}),vec![]))
            })?.result);
        }
        if method == "device.revoke" {
            let id = params["deviceId"].as_str().expect("device");
            let raw: Option<String> = self
                .store
                .connection()
                .query_row("SELECT payload FROM devices WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(|e| errors::storage(e.into()))?;
            let mut device: Device = serde_json::from_str(
                &raw.ok_or_else(|| errors::error(ErrorKind::NotFound, "Device not found"))?,
            )
            .map_err(|_| errors::error(ErrorKind::InternalError, "Invalid device record"))?;
            device.revoked = true;
            let result = self
                .mutate(connection, method, params, |tx| {
                    tx.execute(
                        "UPDATE devices SET payload=?1,revoked=1 WHERE id=?2",
                        params![serde_json::to_string(&device)?, id],
                    )?;
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
                client.outbox.close();
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
            let available: bool = self
                .store
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM pair_windows WHERE code=?1 AND expires_unix>?2)",
                    params![pending.code, Utc::now().timestamp()],
                    |row| row.get(0),
                )
                .map_err(|e| errors::storage(e.into()))?;
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
        let result=self.mutate(connection,method,params,|tx| {
            let mut events=vec![];
            if let Some(device)=&pending.device {
                tx.execute("DELETE FROM pair_windows WHERE code=?1", [&pending.code])?;
                tx.execute("INSERT INTO devices(id,payload,public_key,token_hash) VALUES (?1,?2,?3,?4)",params![device.device_id,serde_json::to_string(device)?,public.as_bytes().as_slice(),auth::token_hash(pending.token.as_ref().expect("token"))])?;
                events.push(("device.paired".into(),json!({"device":device})));
            }
            tx.execute("UPDATE pair_requests SET payload=?1 WHERE id=?2",params![serde_json::to_string(&pending)?,id])?;
            Ok((json!({"requestId":id,"approved":approve}),events))
        })?.result;
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
