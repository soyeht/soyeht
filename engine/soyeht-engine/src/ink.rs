//! Handwriting recognition (ADR 0158, `docs/contracts/14-handwriting-recognition.md`).
//!
//! `ink.recognize` checks the request under the Core lock, which is a bounded walk over the
//! points it carries, and does everything else off it: drawing the strokes and asking the
//! system's recognizer, which takes milliseconds to seconds. The strokes and the text are never
//! stored, logged or kept after the reply.

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    offload::{Offload, SharedPort},
};
use serde::Deserialize as _;
use serde_json::Value;
use soyeht_ink::{Recognizer, Writing};
use soyeht_protocol::InkRecognizeParams;
use std::time::Duration;

/// The longest one recognition may take, a little under the clients' request timeout so a
/// slow one is answered by the engine and not by the client giving up.
const RECOGNIZE_TIMEOUT: Duration = Duration::from_secs(10);

/// The recognizer of this engine, or why it has none.
pub struct Service {
    recognizer: std::result::Result<SharedPort<dyn Recognizer>, soyeht_ink::Error>,
}

impl Service {
    pub fn new(recognizer: soyeht_ink::Result<Box<dyn Recognizer>>) -> Self {
        Self {
            recognizer: recognizer.map(SharedPort::new),
        }
    }

    /// Whether this engine can read handwriting: what `hello` tells the clients.
    pub fn available(&self) -> bool {
        self.recognizer.is_ok()
    }
}

impl Core {
    pub(crate) fn ink_recognize(&self, params: &Value) -> Result<Outcome> {
        let params = InkRecognizeParams::deserialize(params)
            .map_err(|_| errors::invalid("Invalid ink recognition parameters"))?;
        let writing = Writing::from_params(&params)?;
        let recognizer = self.ink.recognizer.clone()?;
        Ok(Outcome::Offload(
            Offload::new(
                "Handwriting recognition timed out",
                RECOGNIZE_TIMEOUT,
                move || {
                    recognizer.with(|recognizer| {
                        soyeht_ink::recognize(recognizer, &writing).map_err(errors::Error::from)
                    })
                },
                |_core, result| {
                    let answer = serde_json::to_value(result?)
                        .map_err(|_| errors::internal("Ink result serialization failed"))?;
                    Ok(Outcome::Reply(answer))
                },
            )
            .concurrent(),
        ))
    }
}
