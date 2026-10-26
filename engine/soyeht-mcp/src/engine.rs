use crate::{Error, Result};
use serde_json::{Value, json};
use soyeht_platform::local::{Stream, connect};
use soyeht_protocol::{
    AttachmentRef, Role, Welcome,
    attachment::AttachmentAssembler,
    frame::{Frame, read_json, write_json},
    handshake, limits, method,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub trait EnginePort {
    fn role(&self) -> &Role;
    fn request(&mut self, method: &str, params: Value) -> Result<Value>;
    fn request_with_attachment(&mut self, method: &str, params: Value) -> Result<EngineResponse> {
        self.request(method, params)
            .map(EngineResponse::without_attachment)
    }
}

pub struct EngineAttachment {
    pub reference: AttachmentRef,
    pub bytes: Vec<u8>,
}

pub struct EngineResponse {
    pub value: Value,
    pub attachment: Option<EngineAttachment>,
}

impl EngineResponse {
    fn without_attachment(value: Value) -> Self {
        Self {
            value,
            attachment: None,
        }
    }
}

pub struct Client {
    socket_path: PathBuf,
    binding: String,
    socket: Stream,
    next_id: u64,
    role: Role,
}

impl Client {
    pub fn connect(socket: &Path, binding: &str) -> Result<Self> {
        let mut client = Self {
            socket_path: socket.to_owned(),
            binding: binding.to_owned(),
            socket: open(socket)?,
            next_id: 0,
            role: Role::Agent,
        };
        client.handshake()?;
        Ok(client)
    }

    fn handshake(&mut self) -> Result<()> {
        handshake::expect_challenge(&self.read()?)?;
        let hello = serde_json::to_value(handshake::hello(
            "soyeht-mcp",
            env!("CARGO_PKG_VERSION"),
            Some(&self.binding),
        ))?;
        let welcome: Welcome = self.request_typed("hello", hello)?;
        handshake::expect_welcome(&welcome, true)?;
        self.role = welcome.role;
        Ok(())
    }

    fn reconnect(&mut self) -> Result<()> {
        self.socket = open(&self.socket_path)?;
        self.handshake()
    }

    fn read(&mut self) -> Result<Value> {
        read_json(&mut self.socket)?.ok_or(Error::UnexpectedFrame)
    }

    fn request_typed<T: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<T> {
        let value = self.request_value(method, params)?;
        Ok(serde_json::from_value(value)?)
    }

    fn request_value(&mut self, method: &str, params: Value) -> Result<Value> {
        self.request_response(method, params)
            .map(|response| response.value)
    }

    fn request_response(&mut self, method: &str, params: Value) -> Result<EngineResponse> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(Error::RequestIdExhausted)?;
        let id = self.next_id;
        write_json(
            &mut self.socket,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )?;
        let mut attachment = AttachmentAssembler::new(limits::WEB_SCREENSHOT_BYTES)?;
        loop {
            let response: Value =
                match Frame::read_from(&mut self.socket)?.ok_or(Error::UnexpectedFrame)? {
                    Frame::Attachment { header, bytes } => {
                        attachment.push(header, &bytes)?;
                        continue;
                    }
                    Frame::Json(bytes) => serde_json::from_slice(&bytes)?,
                    Frame::Terminal { .. } => return Err(Error::UnexpectedFrame),
                };
            if response["id"] != id {
                if attachment.is_pending() {
                    return Err(Error::UnexpectedFrame);
                }
                continue;
            }
            if let Some(error) = response.get("error") {
                if attachment.is_pending() {
                    return Err(Error::UnexpectedFrame);
                }
                return Err(Error::Remote(serde_json::from_value(error.clone())?));
            }
            let value = response
                .get("result")
                .cloned()
                .ok_or(Error::ResponseMissingResult)?;
            let reference = response_attachment(method, &value)?;
            let completed = match reference {
                Some(reference) => Some(EngineAttachment {
                    bytes: attachment.commit(&reference)?,
                    reference,
                }),
                None if attachment.is_pending() => return Err(Error::UnexpectedFrame),
                None => None,
            };
            return Ok(EngineResponse {
                value,
                attachment: completed,
            });
        }
    }
}

fn response_attachment(method: &str, value: &Value) -> Result<Option<AttachmentRef>> {
    let field = match method {
        method::WEB_SCREENSHOT | method::WEB_READ => Some("image"),
        method::WEB_SNAPSHOT => Some("screenshot"),
        _ => None,
    };
    let reference: Option<AttachmentRef> = field
        .and_then(|field| value.get(field))
        .map(|value| serde_json::from_value(value.clone()).map_err(Error::from))
        .transpose()?;
    if matches!(method, method::WEB_SCREENSHOT | method::WEB_READ) && reference.is_none() {
        return Err(Error::ResponseMissingAttachment);
    }
    if let Some(reference) = &reference
        && (reference.mime_type != "image/png" || reference.name != "screenshot.png")
    {
        return Err(Error::UnexpectedFrame);
    }
    Ok(reference)
}

impl EnginePort for Client {
    fn role(&self) -> &Role {
        &self.role
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        match self.request_value(method, params.clone()) {
            Err(error) if reconnectable(&error) => {
                self.reconnect()?;
                self.request_value(method, params)
            }
            result => result,
        }
    }

    fn request_with_attachment(&mut self, method: &str, params: Value) -> Result<EngineResponse> {
        match self.request_response(method, params.clone()) {
            Err(error) if reconnectable(&error) => {
                self.reconnect()?;
                self.request_response(method, params)
            }
            result => result,
        }
    }
}

fn open(socket: &Path) -> Result<Stream> {
    let stream = connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    Ok(stream)
}

fn reconnectable(error: &Error) -> bool {
    matches!(
        error,
        Error::Io(_) | Error::Protocol(_) | Error::UnexpectedFrame
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_platform::local::Listener;
    use soyeht_protocol::{
        attachment::attachment_header,
        frame::{Frame, read_json, write_json},
        profile::DEV_ENGINE_SOCKET,
    };
    use std::thread;

    /// Where the endpoints of these tests live. A Unix socket path is short-limited, and the
    /// temporary folder of macOS is too long for one.
    fn scratch() -> PathBuf {
        if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        }
    }

    #[test]
    fn framed_engine_connection_proves_the_binding_and_preserves_remote_errors() {
        let directory = scratch().join(format!("mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join(DEV_ENGINE_SOCKET);
        let listener = Listener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            write_json(&mut stream, &json!({"jsonrpc":"2.0","method":"auth.challenge","params":{"eventId":"0","at":"now","nonce":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","engineId":"engine"}})).unwrap();
            let hello: Value = read_json(&mut stream).unwrap().unwrap();
            assert_eq!(hello["params"]["binding"], "secret");
            let id = hello["id"].clone();
            write_json(&mut stream, &json!({"jsonrpc":"2.0","id":id,"result":{"engineVersion":"1","protocolVersion":1,"connectionId":"c","role":"agent","capabilities":[],"dbId":"engine"}})).unwrap();
            let call: Value = read_json(&mut stream).unwrap().unwrap();
            assert_eq!(call["method"], "task.next");
            let id = call["id"].clone();
            write_json(&mut stream, &json!({"jsonrpc":"2.0","id":id,"error":{"code":-32010,"message":"stale","data":{"kind":"stale_attempt"}}})).unwrap();
        });
        let mut client = Client::connect(&socket, "secret").unwrap();
        assert_eq!(client.role(), &Role::Agent);
        let error = client.request("task.next", json!({})).unwrap_err();
        assert!(
            matches!(error, Error::Remote(error) if error.data.kind == soyeht_protocol::ErrorKind::StaleAttempt)
        );
        server.join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn request_reconnects_after_engine_restart_and_reuses_idempotency_key() {
        let directory = scratch().join(format!("mcp-restart-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join(DEV_ENGINE_SOCKET);
        let listener = Listener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let mut first = accept_client(&listener);
            let original: Value = read_json(&mut first).unwrap().unwrap();
            assert_eq!(original["method"], "task.next");
            drop(first);

            let mut second = accept_client(&listener);
            let repeated: Value = read_json(&mut second).unwrap().unwrap();
            assert_eq!(repeated["method"], original["method"]);
            assert_eq!(repeated["params"], original["params"]);
            let id = repeated["id"].clone();
            write_json(
                &mut second,
                &json!({"jsonrpc":"2.0","id":id,"result":{"empty":true}}),
            )
            .unwrap();
        });
        let mut client = Client::connect(&socket, "secret").unwrap();
        let result = client
            .request(
                "task.next",
                json!({"idempotencyKey":"same-key","paneId":"pane"}),
            )
            .unwrap();
        assert_eq!(result, json!({"empty":true}));
        server.join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn screenshot_bytes_are_committed_only_by_the_matching_response_reference() {
        let directory = scratch().join(format!("mcp-image-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join(DEV_ENGINE_SOCKET);
        let listener = Listener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let mut stream = accept_client(&listener);
            let request: Value = read_json(&mut stream).unwrap().unwrap();
            let id = request["id"].clone();
            let attachment_id = "00112233-4455-4677-8899-aabbccddeeff";
            Frame::Attachment {
                header: attachment_header(attachment_id).unwrap(),
                bytes: b"png-bytes".to_vec(),
            }
            .write_to(&mut stream)
            .unwrap();
            write_json(
                &mut stream,
                &json!({"jsonrpc":"2.0","id":id,"result":{"paneId":"web","image":{
                    "attachmentId":attachment_id,"mimeType":"image/png",
                    "name":"screenshot.png","bytes":"9"
                }}}),
            )
            .unwrap();
        });
        let mut client = Client::connect(&socket, "secret").unwrap();
        let response = client
            .request_with_attachment("web.screenshot", json!({"paneId":"web"}))
            .unwrap();
        let attachment = response.attachment.unwrap();
        assert_eq!(attachment.bytes, b"png-bytes");
        assert_eq!(attachment.reference.mime_type, "image/png");
        server.join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn accept_client(listener: &Listener) -> Stream {
        let mut stream = listener.accept().unwrap();
        write_json(
            &mut stream,
            &json!({
                "jsonrpc":"2.0",
                "method":"auth.challenge",
                "params":{
                    "eventId":"0",
                    "at":"now",
                    "nonce":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                    "engineId":"engine"
                }
            }),
        )
        .unwrap();
        let hello: Value = read_json(&mut stream).unwrap().unwrap();
        assert_eq!(hello["method"], "hello");
        assert_eq!(hello["params"]["binding"], "secret");
        let id = hello["id"].clone();
        write_json(
            &mut stream,
            &json!({
                "jsonrpc":"2.0",
                "id":id,
                "result":{
                    "engineVersion":"1",
                    "protocolVersion":1,
                    "connectionId":"c",
                    "role":"agent",
                    "capabilities":[],
                    "dbId":"engine"
                }
            }),
        )
        .unwrap();
        stream
    }
}
