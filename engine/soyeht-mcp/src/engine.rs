use crate::{Error, Result};
use serde_json::{Value, json};
use soyeht_protocol::{
    AuthChallengeEvent, Role, Welcome,
    frame::{read_json, write_json},
};
use std::{
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

pub trait EnginePort {
    fn role(&self) -> &Role;
    fn request(&mut self, method: &str, params: Value) -> Result<Value>;
}

pub struct Client {
    socket_path: PathBuf,
    binding: String,
    socket: UnixStream,
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
        let challenge = self.read()?;
        if challenge["method"] != "auth.challenge" {
            return Err(Error::EngineChallengeExpected);
        }
        let _: AuthChallengeEvent = serde_json::from_value(challenge["params"].clone())?;
        let welcome: Welcome = self.request_typed(
            "hello",
            json!({
                "client":"soyeht-mcp",
                "clientVersion":env!("CARGO_PKG_VERSION"),
                "protocolVersion":1,
                "binding":self.binding.clone()
            }),
        )?;
        if welcome.protocol_version != 1 || !matches!(welcome.role, Role::Agent | Role::Chief) {
            return Err(Error::UnexpectedHandshake);
        }
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
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(Error::RequestIdExhausted)?;
        let id = self.next_id;
        write_json(
            &mut self.socket,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )?;
        loop {
            let response = self.read()?;
            if response["id"] != id {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(Error::Remote(serde_json::from_value(error.clone())?));
            }
            return response
                .get("result")
                .cloned()
                .ok_or(Error::ResponseMissingResult);
        }
    }
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
}

fn open(socket: &Path) -> Result<UnixStream> {
    let stream = UnixStream::connect(socket)?;
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
    use soyeht_protocol::{
        frame::{read_json, write_json},
        profile::DEV_ENGINE_SOCKET,
    };
    use std::{os::unix::net::UnixListener, thread};

    #[test]
    fn framed_engine_connection_proves_the_binding_and_preserves_remote_errors() {
        let directory = std::path::Path::new("/tmp").join(format!("mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join(DEV_ENGINE_SOCKET);
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
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
        let directory =
            std::path::Path::new("/tmp").join(format!("mcp-restart-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join(DEV_ENGINE_SOCKET);
        let listener = UnixListener::bind(&socket).unwrap();
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

    fn accept_client(listener: &UnixListener) -> UnixStream {
        let (mut stream, _) = listener.accept().unwrap();
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
