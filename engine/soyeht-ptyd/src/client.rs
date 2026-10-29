use crate::{
    Error, Result,
    wire::{Request, Response, VERSION},
};
use soyeht_platform::local::{Stream, connect};
use soyeht_protocol::frame::{read_json, write_json};
use std::path::Path;
use std::time::Duration;

/// How long a request to the supervisor waits to be written or answered. A caller that is
/// not to be held up by it waits off its locks (the engine's Core lock above all).
pub const IO_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Client {
    stream: Stream,
    pub supervisor_id: String,
    pub supervisor_generation: u32,
}

impl Client {
    pub fn connect(socket: &Path, engine_generation: u64) -> Result<Self> {
        let stream = connect(socket)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
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
            Response::Error { kind, message } => Err(Error::Remote { kind, message }),
            _ => Err(Error::InvalidWelcome),
        }
    }

    pub fn request(&mut self, request: Request) -> Result<Response> {
        write_json(&mut self.stream, &request)?;
        read_json(&mut self.stream)?.ok_or(Error::InvalidResponse)
    }
}
