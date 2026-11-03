//! Pairing a device with an engine (ADR 0013, 0132): begin, check that the engine holds the key the
//! link named, prove the code, and wait for the person to allow access on the computer.

use super::{
    Credential, PairingFailure, PairingLink, bytes32,
    credential::{engine_verifying_key, verify},
};
use crate::{Error, Result, transport::Link};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{SigningKey, VerifyingKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::{
    EventName, PairBeginParams, PairBeginResult, PairProveParams, PairResultEvent, Request,
    client_policy, device_auth, frame::Frame, handshake, limits,
};
use std::{
    io::{Read, Write},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// The engine's pairing window.
const WINDOW: Duration = Duration::from_secs(limits::PAIRING_WINDOW_SECONDS as u64);
/// A request the engine does not answer in this time ends the attempt.
const REQUEST_DEADLINE: Duration = Duration::from_millis(client_policy::REQUEST_TIMEOUT_MS);
/// Connections tried before the pairing is given up: the tailnet can take a moment to carry a packet.
const ATTEMPTS: usize = 3;

/// What the person should see while the pairing waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// The fingerprint of this device's key, to compare with the one the computer shows.
    Fingerprint(String),
    /// The engine has the proof and waits for the person to allow access.
    WaitingForApproval,
}

/// Pairs with the engine `text` names, calling the device `name`. Blocks until the person decides on
/// the computer, the window expires, or the attempt fails. `stop` is polled and ends the wait early.
pub fn pair(
    text: &str,
    name: &str,
    progress: &mut dyn FnMut(Progress),
    stopped: &dyn Fn() -> bool,
) -> Result<Credential> {
    let link = super::parse_link(text).map_err(Error::Pairing)?;
    if name.is_empty() || name.len() > limits::DEVICE_NAME_BYTES {
        return Err(Error::Pairing(PairingFailure::InvalidName));
    }
    let mut seed = [0_u8; 32];
    getrandom::fill(&mut seed).map_err(|_| Error::Invalid("the system has no random numbers"))?;
    let key = SigningKey::from_bytes(&seed);
    let public = key.verifying_key();
    progress(Progress::Fingerprint(fingerprint(&public)));
    let begin = PairBeginParams {
        code: link.code.clone(),
        device_name: name.to_owned(),
        device_public_key: STANDARD.encode(public.as_bytes()),
        idempotency_key: Uuid::new_v4().to_string(),
    };
    let mut pairing = Pairing {
        link: &link,
        name,
        key: &key,
        begin,
        prove: None,
        progress,
        stopped,
        deadline: Instant::now() + WINDOW,
    };
    let mut attempt = 0;
    loop {
        attempt += 1;
        match pairing.once() {
            Ok(credential) => return Ok(credential),
            Err(Error::Io(_) | Error::Disconnected) if attempt < ATTEMPTS && !stopped() => {
                std::thread::sleep(Duration::from_secs(1));
            }
            Err(error) => return Err(error),
        }
    }
}

/// SHA-256 of the key, in hex: what both screens show for the person to compare.
fn fingerprint(key: &VerifyingKey) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One pairing, which may take more than one connection: the proof made on the first is the one the
/// later ones send, so that the engine's answer to a repeated request is the same.
struct Pairing<'a> {
    link: &'a PairingLink,
    name: &'a str,
    key: &'a SigningKey,
    begin: PairBeginParams,
    prove: Option<PairProveParams>,
    progress: &'a mut dyn FnMut(Progress),
    stopped: &'a dyn Fn() -> bool,
    deadline: Instant,
}

impl Pairing<'_> {
    fn once(&mut self) -> Result<Credential> {
        let Link {
            mut reader,
            writer,
            close,
        } = super::connect(self.link.address)?;
        let _closing = Closing(Arc::clone(&close));
        let _window = Watch::over(&close, self.deadline);
        let mut wire = Wire {
            reader: &mut reader,
            writer,
            close: &close,
            results: Vec::new(),
            next: 0,
        };
        wire.greeting(&self.link.engine_id)?;
        let answer: PairBeginResult =
            serde_json::from_value(wire.call(&Request::PairBegin(self.begin.clone()))?)?;
        let challenge =
            bytes32(&answer.challenge).ok_or(Error::Pairing(PairingFailure::Unexpected))?;
        let engine_key = engine_verifying_key(&self.link.engine_key)
            .ok_or(Error::Pairing(PairingFailure::InvalidLink))?;
        let device_key = self.key.verifying_key();
        // Only the engine whose key the link named can sign this, and the device proves the code
        // only after it has (ADR 0132).
        let signed =
            device_auth::engine_pair_bytes(&answer.request_id, &challenge, device_key.as_bytes());
        let proof = answer
            .engine_proof
            .as_deref()
            .ok_or(Error::Pairing(PairingFailure::EngineTooOld))?;
        verify(&engine_key, &signed, proof)?;
        if self.prove.is_none() {
            let mac = device_auth::pair_proof(
                &self.link.code,
                device_key.as_bytes(),
                &challenge,
                self.name,
            )
            .ok_or(Error::Invalid("the pairing proof could not be made"))?;
            self.prove = Some(PairProveParams {
                request_id: answer.request_id.clone(),
                proof: mac,
                idempotency_key: Uuid::new_v4().to_string(),
            });
        }
        let prove = self
            .prove
            .clone()
            .filter(|prove| prove.request_id == answer.request_id)
            .ok_or(Error::Pairing(PairingFailure::Unexpected))?;
        wire.call(&Request::PairProve(prove))?;
        (self.progress)(Progress::WaitingForApproval);
        let result = wire.result(&answer.request_id, self.stopped, self.deadline)?;
        match (result.approved, result.device_token, result.device_id) {
            (true, Some(token), Some(device)) => {
                Ok(Credential::made(self.link, device, token, self.key))
            }
            _ => Err(Error::Pairing(PairingFailure::Refused(
                result
                    .reason
                    .unwrap_or_else(|| "The computer did not allow access.".to_owned()),
            ))),
        }
    }
}

/// Closes the connection when an attempt ends, however it ends: nothing else would, and the engine
/// would wait on it.
struct Closing(Arc<dyn Fn() + Send + Sync>);

impl Drop for Closing {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// Ends the connection when the pairing window runs out, so that no read waits for ever.
struct Watch(mpsc::Sender<()>);

impl Watch {
    fn over(close: &Arc<dyn Fn() + Send + Sync>, deadline: Instant) -> Self {
        let (done, wait) = mpsc::channel::<()>();
        let close = Arc::clone(close);
        std::thread::spawn(move || {
            if wait.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                close();
            }
        });
        Self(done)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // The thread may have ended already, and then there is nothing left to stop.
        self.0.send(()).unwrap_or_default();
    }
}

/// The exchange on one connection, a request and its answer at a time.
struct Wire<'a> {
    reader: &'a mut Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    close: &'a Arc<dyn Fn() + Send + Sync>,
    /// `pair.result events that arrived while a request was waiting for its answer.
    results: Vec<PairResultEvent>,
    next: u64,
}

impl Wire<'_> {
    fn push_result(&mut self, message: &Value) -> Result<()> {
        if message.get("method").and_then(Value::as_str) == Some(EventName::PairResult.as_str()) {
            self.results
                .push(serde_json::from_value(message["params"].clone())?);
        }
        Ok(())
    }

    fn frame(&mut self) -> Result<Value> {
        match Frame::read_from(&mut *self.reader)? {
            Some(Frame::Json(bytes)) => Ok(serde_json::from_slice(&bytes)?),
            Some(_) => Err(Error::Pairing(PairingFailure::Unexpected)),
            None => Err(Error::Disconnected),
        }
    }

    fn greeting(&mut self, engine_id: &str) -> Result<()> {
        let greeting = self.frame()?;
        let challenge = handshake::expect_challenge(&greeting)?;
        if challenge.engine_id == engine_id {
            Ok(())
        } else {
            Err(Error::Pairing(PairingFailure::Unexpected))
        }
    }

    fn call(&mut self, request: &Request) -> Result<Value> {
        let Value::Object(mut body) = serde_json::to_value(request)? else {
            return Err(Error::Invalid("request is not an object"));
        };
        self.next += 1;
        body.insert("jsonrpc".into(), json!("2.0"));
        body.insert("id".into(), json!(self.next));
        Frame::Json(serde_json::to_vec(&Value::Object(body))?).write_to(&mut self.writer)?;
        self.writer.flush()?;
        // The engine answers at once; a connection that does not is replaced.
        let _deadline = Watch::over(self.close, Instant::now() + REQUEST_DEADLINE);
        loop {
            let message = self.frame()?;
            self.push_result(&message)?;
            if message.get("id").and_then(Value::as_u64) != Some(self.next) {
                continue;
            }
            return match message.get("error") {
                Some(error) => Err(Error::Remote(serde_json::from_value(error.clone())?)),
                None => Ok(message["result"].clone()),
            };
        }
    }

    /// The person's answer for `request_id`.
    fn result(
        &mut self,
        request_id: &str,
        stopped: &dyn Fn() -> bool,
        deadline: Instant,
    ) -> Result<PairResultEvent> {
        loop {
            if let Some(found) = self.results.iter().position(|r| r.request_id == request_id) {
                return Ok(self.results.swap_remove(found));
            }
            if Instant::now() >= deadline {
                return Err(Error::Pairing(PairingFailure::Expired));
            }
            if stopped() {
                return Err(Error::Disconnected);
            }
            let message = self.frame()?;
            self.push_result(&message)?;
        }
    }
}
