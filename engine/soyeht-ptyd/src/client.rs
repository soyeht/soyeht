use crate::wire::{Request, Response, VERSION};
use soyeht_protocol::frame::Frame;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

pub struct Client {
    stream: UnixStream,
    pub supervisor_id: String,
    pub supervisor_generation: u32,
}

impl Client {
    pub fn connect(socket: &Path, engine_generation: u64) -> io::Result<Self> {
        let stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut client = Self {
            stream,
            supervisor_id: String::new(),
            supervisor_generation: 0,
        };
        match client.request(Request::Hello {
            version: VERSION,
            engine_generation,
        })? {
            Response::Welcome {
                version: VERSION,
                supervisor_id,
                supervisor_generation,
            } => {
                client.supervisor_id = supervisor_id;
                client.supervisor_generation = supervisor_generation;
                Ok(client)
            }
            Response::Error { kind, .. } => Err(io::Error::other(kind)),
            _ => Err(io::Error::other("invalid ptyd welcome")),
        }
    }

    pub fn request(&mut self, request: Request) -> io::Result<Response> {
        Frame::Json(serde_json::to_vec(&request)?).write_to(&mut self.stream)?;
        match Frame::read_from(&mut self.stream)? {
            Some(Frame::Json(bytes)) => Ok(serde_json::from_slice(&bytes)?),
            _ => Err(io::Error::other("invalid ptyd response")),
        }
    }
}
