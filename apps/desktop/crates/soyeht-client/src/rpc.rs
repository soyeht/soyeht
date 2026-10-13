//! Request/response correlation over one connection. A caller blocks on `call`; the reader
//! thread completes the matching request when the response frame arrives.

use crate::attachments::Upload;
use crate::{Error, Result, transport::Closer};
use serde_json::{Value, json};
use soyeht_protocol::{Request, attachment::attachment_frames, client_policy, frame::write_json};
use std::{
    collections::HashMap,
    io::Write,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

/// How long a request may wait for its answer: the contract's, as on the Mac client.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(client_policy::REQUEST_TIMEOUT_MS);

/// What the reader must do besides delivering a successful result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    /// The result describes a stream the reader must start tracking before more frames arrive.
    StreamSubscribe,
}

pub struct Pending {
    pub kind: Kind,
    reply: mpsc::Sender<Result<Value>>,
}

impl Pending {
    /// Hands the caller its result. False when the caller timed out and left, which is not an
    /// error: nobody is waiting any more.
    pub fn complete(self, result: Result<Value>) -> bool {
        self.reply.send(result).is_ok()
    }
}

pub struct Rpc {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    close: Closer,
}

impl Rpc {
    /// `first_id` continues numbering after the handshake requests.
    pub fn new(writer: Box<dyn Write + Send>, close: Closer, first_id: u64) -> Self {
        Self {
            writer: Mutex::new(writer),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(first_id),
            close,
        }
    }

    /// Sends a typed request and waits for its result.
    pub fn call(&self, request: &Request) -> Result<Value> {
        self.call_with_uploads(request, &[])
    }

    /// As [`Rpc::call`], for a request that names attachments: their bytes go first, as binary
    /// frames, with nothing between them and the request.
    pub fn call_with_uploads(&self, request: &Request, uploads: &[Upload]) -> Result<Value> {
        let mut frames = Vec::new();
        for upload in uploads {
            frames.extend(attachment_frames(&upload.reference, &upload.bytes)?);
        }
        let kind = match request {
            Request::PaneStreamSubscribe(_) => Kind::StreamSubscribe,
            _ => Kind::Plain,
        };
        let (method, message, id) = self.envelope(request)?;
        let (reply, answer) = mpsc::channel();
        self.lock_pending().insert(id, Pending { kind, reply });
        let sent = {
            let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            frames
                .iter()
                .try_for_each(|frame| frame.write_to(&mut *writer))
                .and_then(|()| write_json(&mut *writer, &message))
                .and_then(|()| Ok(writer.flush()?))
        };
        if let Err(error) = sent {
            self.lock_pending().remove(&id);
            return Err(error.into());
        }
        match answer.recv_timeout(REQUEST_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.lock_pending().remove(&id);
                // A connection that stops answering is replaced, as the Mac client does.
                (self.close)();
                Err(Error::Timeout(method))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::Disconnected),
        }
    }

    fn envelope(&self, request: &Request) -> Result<(String, Value, u64)> {
        let Value::Object(mut body) = serde_json::to_value(request)? else {
            return Err(Error::Invalid("request is not an object"));
        };
        let method = body
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        body.insert("jsonrpc".into(), json!("2.0"));
        body.insert("id".into(), json!(id));
        body.entry("params").or_insert_with(|| json!({}));
        Ok((method, Value::Object(body), id))
    }

    pub fn take(&self, id: u64) -> Option<Pending> {
        self.lock_pending().remove(&id)
    }

    /// The connection is gone: every waiting caller learns it.
    pub fn fail_all(&self) {
        let waiting: Vec<Pending> = self.lock_pending().drain().map(|(_, p)| p).collect();
        for pending in waiting {
            pending.complete(Err(Error::Disconnected));
        }
    }

    pub fn close(&self) {
        (self.close)();
    }

    fn lock_pending(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Pending>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
