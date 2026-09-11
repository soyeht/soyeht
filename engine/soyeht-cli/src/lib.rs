#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use soyeht_protocol::{
    AuthChallengeEvent, SnapshotSection, StateSnapshotPartEvent, StateSnapshotResult, Welcome,
    frame::Frame,
};
use std::{
    collections::BTreeMap,
    io,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Client {
    socket: UnixStream,
    next: u64,
    directory: PathBuf,
}
pub struct Snapshot {
    pub metadata: StateSnapshotResult,
    pub sections: BTreeMap<String, Vec<Value>>,
}

impl Client {
    pub fn connect(directory: &Path) -> io::Result<Self> {
        let socket = UnixStream::connect(directory.join("engine.sock"))?;
        socket.set_read_timeout(Some(Duration::from_secs(10)))?;
        socket.set_write_timeout(Some(Duration::from_secs(10)))?;
        let mut client = Self {
            socket,
            next: 0,
            directory: directory.into(),
        };
        let challenge = client.read()?;
        if challenge["method"] != "auth.challenge" {
            return Err(io::Error::other(
                "engine did not send an authentication challenge",
            ));
        }
        let _: AuthChallengeEvent = serde_json::from_value(challenge["params"].clone())?;
        let welcome:Welcome=client.request("hello",json!({"client":"soyeht-cli","clientVersion":env!("CARGO_PKG_VERSION"),"protocolVersion":1}))?;
        if welcome.protocol_version != 1 || welcome.role != soyeht_protocol::Role::HumanLocal {
            return Err(io::Error::other("unexpected engine handshake"));
        }
        Ok(client)
    }
    fn read(&mut self) -> io::Result<Value> {
        match Frame::read_from(&mut self.socket)? {
            Some(Frame::Json(bytes)) => Ok(serde_json::from_slice(&bytes)?),
            _ => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "engine closed or sent an unexpected binary frame",
            )),
        }
    }
    fn begin(&mut self, method: &str, params: Value) -> io::Result<u64> {
        soyeht_protocol::validate::parameters(method, &params).map_err(io::Error::other)?;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| io::Error::other("request identifiers exhausted"))?;
        Frame::Json(serde_json::to_vec(
            &json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params}),
        )?)
        .write_to(&mut self.socket)?;
        Ok(self.next)
    }
    pub fn request<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> io::Result<T> {
        let mut reconnect = false;
        for attempt in 0..4 {
            if reconnect {
                std::thread::sleep(Duration::from_millis(500 * attempt));
                match Self::connect(&self.directory) {
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
        Err(io::Error::other("request attempts exhausted"))
    }

    fn request_once<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> io::Result<T> {
        let id = self.begin(method, params)?;
        loop {
            let response = self.read()?;
            if response["id"] == id {
                return result(response);
            }
        }
    }
    pub fn snapshot(&mut self) -> io::Result<Snapshot> {
        let id = self.begin("state.snapshot", json!({}))?;
        let mut sections: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut identity = None;
        let mut count = 0u32;
        let mut total = None;
        loop {
            let response = self.read()?;
            if response["id"] == id {
                let metadata: StateSnapshotResult = result(response)?;
                if Some(&metadata.snapshot_id) != identity.as_ref()
                    || metadata.parts != count
                    || Some(count) != total
                {
                    return Err(io::Error::other("incomplete snapshot"));
                }
                return Ok(Snapshot { metadata, sections });
            }
            if response["method"] != "state.snapshot_part" {
                continue;
            }
            let part: StateSnapshotPartEvent = serde_json::from_value(response["params"].clone())?;
            if identity
                .as_ref()
                .is_some_and(|prior| prior != &part.snapshot_id)
                || total.is_some_and(|prior| prior != part.of)
                || part.index != count
                || part.index >= part.of
            {
                return Err(io::Error::other("snapshot pages are inconsistent"));
            }
            identity = Some(part.snapshot_id);
            total = Some(part.of);
            count += 1;
            let name = snapshot_section_name(&part.section).to_owned();
            let items = part
                .data
                .as_array()
                .ok_or_else(|| io::Error::other("snapshot section must be an array"))?;
            sections
                .entry(name)
                .or_default()
                .extend(items.iter().cloned());
        }
    }
}

fn snapshot_section_name(section: &SnapshotSection) -> &'static str {
    match section {
        SnapshotSection::Workspaces => "workspaces",
        SnapshotSection::Panes => "panes",
        SnapshotSection::Tasks => "tasks",
        SnapshotSection::Requests => "requests",
        SnapshotSection::Attention => "attention",
        SnapshotSection::Devices => "devices",
        SnapshotSection::Capabilities => "capabilities",
        SnapshotSection::Streams => "streams",
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
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
}

fn result<T: DeserializeOwned>(response: Value) -> io::Result<T> {
    if let Some(error) = response.get("error") {
        return Err(io::Error::other(error.to_string()));
    }
    Ok(serde_json::from_value(
        response
            .get("result")
            .ok_or_else(|| io::Error::other("response has no result"))?
            .clone(),
    )?)
}
