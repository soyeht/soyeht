use crate::{Error, Result};
use serde_json::{Value, json};
use soyeht_protocol::{
    AuthChallengeEvent, Role, Welcome,
    frame::{read_json, write_json},
};
use std::{os::unix::net::UnixStream, path::Path, time::Duration};

pub trait EnginePort {
    fn role(&self) -> &Role;
    fn request(&mut self, method: &str, params: Value) -> Result<Value>;
}

pub struct Client {
    socket: UnixStream,
    next_id: u64,
    role: Role,
}

impl Client {
    pub fn connect(socket: &Path, binding: &str) -> Result<Self> {
        let stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        let mut client = Self {
            socket: stream,
            next_id: 0,
            role: Role::Agent,
        };
        let challenge = client.read()?;
        if challenge["method"] != "auth.challenge" {
            return Err(Error::EngineChallengeExpected);
        }
        let _: AuthChallengeEvent = serde_json::from_value(challenge["params"].clone())?;
        let welcome: Welcome = client.request_typed(
            "hello",
            json!({
                "client":"soyeht-mcp",
                "clientVersion":env!("CARGO_PKG_VERSION"),
                "protocolVersion":1,
                "binding":binding
            }),
        )?;
        if welcome.protocol_version != 1 || !matches!(welcome.role, Role::Agent | Role::Chief) {
            return Err(Error::UnexpectedHandshake);
        }
        client.role = welcome.role;
        Ok(client)
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
        self.request_value(method, params)
    }
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
}
