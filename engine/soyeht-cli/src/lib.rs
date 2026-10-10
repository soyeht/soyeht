#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod error;
pub use error::{Error, Result};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use soyeht_platform::local::{Stream, connect};
use soyeht_protocol::{
    StateSnapshotPartEvent, Welcome,
    frame::{read_json, write_json},
    handshake,
    profile::DEV_ENGINE_SOCKET,
    snapshot::SnapshotAssembler,
};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Client {
    socket: Stream,
    next: u64,
    directory: PathBuf,
    binding: Option<String>,
    client_name: &'static str,
}
pub use soyeht_protocol::snapshot::Snapshot;

impl Client {
    pub fn connect(directory: &Path) -> Result<Self> {
        Self::connect_as(directory, None, "soyeht-cli")
    }

    pub fn connect_bound(directory: &Path, binding: &str) -> Result<Self> {
        Self::connect_as(directory, Some(binding), "soyeht-cli")
    }

    pub fn connect_hook(directory: &Path) -> Result<Self> {
        Self::connect_as(directory, None, "soyeht-hook")
    }

    fn connect_as(
        directory: &Path,
        binding: Option<&str>,
        client_name: &'static str,
    ) -> Result<Self> {
        let socket = connect(&directory.join(DEV_ENGINE_SOCKET))?;
        socket.set_read_timeout(Some(Duration::from_secs(10)))?;
        socket.set_write_timeout(Some(Duration::from_secs(10)))?;
        let mut client = Self {
            socket,
            next: 0,
            directory: directory.into(),
            binding: binding.map(str::to_owned),
            client_name,
        };
        handshake::expect_challenge(&client.read()?)?;
        let hello = serde_json::to_value(handshake::hello(
            client_name,
            env!("CARGO_PKG_VERSION"),
            binding,
        ))?;
        let welcome: Welcome = client.request("hello", hello)?;
        handshake::expect_welcome(&welcome, binding.is_some())?;
        Ok(client)
    }
    fn read(&mut self) -> Result<Value> {
        read_json(&mut self.socket)?.ok_or(Error::UnexpectedFrame)
    }
    fn begin(&mut self, method: &str, params: Value) -> Result<u64> {
        soyeht_protocol::validate::parameters(method, &params)?;
        self.next = self.next.checked_add(1).ok_or(Error::RequestIdExhausted)?;
        write_json(
            &mut self.socket,
            &json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params}),
        )?;
        Ok(self.next)
    }
    pub fn request<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> Result<T> {
        let mut reconnect = false;
        for attempt in 0..4 {
            if reconnect {
                std::thread::sleep(Duration::from_millis(500 * attempt));
                match Self::connect_as(&self.directory, self.binding.as_deref(), self.client_name) {
                    Ok(client) => *self = client,
                    Err(error) if attempt < 3 && retryable(&error) => continue,
                    Err(error) => return Err(error),
                }
            }
            let result = self.request_once(method, params.clone());
            match result {
                Err(error) if method != "hello" && attempt < 3 && retryable(&error) => {
                    reconnect = true
                }
                result => return result,
            }
        }
        Err(Error::AttemptsExhausted)
    }

    fn request_once<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> Result<T> {
        let id = self.begin(method, params)?;
        loop {
            let response = self.read()?;
            if response["id"] == id {
                return result(response);
            }
        }
    }
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let id = self.begin("state.snapshot", json!({}))?;
        let mut assembler = SnapshotAssembler::default();
        loop {
            let response = self.read()?;
            if response["id"] == id {
                return Ok(assembler.finish(result(response)?)?);
            }
            if response["method"] == "state.snapshot_part" {
                let part: StateSnapshotPartEvent =
                    serde_json::from_value(response["params"].clone())?;
                assembler.push(part)?;
            }
        }
    }
}

fn retryable(error: &Error) -> bool {
    matches!(
        error.io_kind(),
        Some(
            io::ErrorKind::UnexpectedEof
                | io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionRefused
                | io::ErrorKind::NotConnected
                | io::ErrorKind::NotFound
                | io::ErrorKind::TimedOut
                | io::ErrorKind::WouldBlock
        )
    )
}

fn result<T: DeserializeOwned>(response: Value) -> Result<T> {
    if let Some(error) = response.get("error") {
        return Err(Error::Remote(serde_json::from_value(error.clone())?));
    }
    Ok(serde_json::from_value(
        response
            .get("result")
            .ok_or(Error::ResponseMissingResult)?
            .clone(),
    )?)
}
