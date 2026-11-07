use crate::{
    Error, Result,
    wire::{FEATURE_WATCH, Readable, Request, Response, Role, VERSION, Watched},
    wire_kind,
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
    /// What the supervisor said it can do beyond the first protocol (`Welcome.features`). An
    /// older one says nothing.
    pub features: Vec<String>,
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
            features: Vec::new(),
        };
        match client.request(Request::Hello {
            version: VERSION,
            engine_generation,
            role: Role::Request,
        })? {
            Response::Welcome {
                version: VERSION,
                supervisor_id,
                supervisor_generation,
                features,
            } => {
                client.supervisor_id = supervisor_id;
                client.supervisor_generation = supervisor_generation;
                client.features = features;
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

/// A connection that only receives notices (the watch role). The request connection of the same
/// engine generation must be made first.
pub struct Watcher {
    stream: Stream,
}

impl Watcher {
    /// Opens a watch connection. `Error::WatchUnavailable` says the supervisor does not know the
    /// role (it is older than this engine), and the engine goes on polling.
    pub fn connect(socket: &Path, engine_generation: u64) -> Result<Self> {
        let mut stream = connect(socket)?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        // The welcome of a supervisor that is stopped is not waited for for ever.
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        write_json(
            &mut stream,
            &Request::Hello {
                version: VERSION,
                engine_generation,
                role: Role::Watch,
            },
        )?;
        match read_json(&mut stream)?.ok_or(Error::InvalidWelcome)? {
            Response::Welcome {
                version: VERSION,
                features,
                ..
            } if features.iter().any(|feature| feature == FEATURE_WATCH) => {
                stream.set_read_timeout(None)?;
                Ok(Self { stream })
            }
            // An older supervisor ignores the role: it answers as to a second request connection
            // (refused) or welcomes it with no features.
            Response::Welcome { .. } => Err(Error::WatchUnavailable),
            Response::Error { kind, .. } if kind == wire_kind::ENGINE_CONNECTED => {
                Err(Error::WatchUnavailable)
            }
            Response::Error { kind, message } => Err(Error::Remote { kind, message }),
            _ => Err(Error::InvalidWelcome),
        }
    }

    /// Says what the engine has read to. The answer lists the processes with more to read.
    pub fn watch(&mut self, instances: Vec<Watched>) -> Result<Vec<Readable>> {
        write_json(&mut self.stream, &Request::Watch { instances })?;
        loop {
            match self.notice()? {
                Notice::Level(readable) => return Ok(readable),
                // A notice that comes before the answer is in the level or comes again.
                Notice::Readable(_) => {}
            }
        }
    }

    /// Waits for the next notice, which is what arrives when a process has output. Blocks until one comes or the connection ends.
    pub fn notice(&mut self) -> Result<Notice> {
        match read_json(&mut self.stream)?.ok_or(Error::InvalidResponse)? {
            Response::Watching(readable) => Ok(Notice::Level(readable)),
            Response::Readable(readable) => Ok(Notice::Readable(readable)),
            Response::Error { kind, message } => Err(Error::Remote { kind, message }),
            _ => Err(Error::InvalidResponse),
        }
    }

    /// Ends the connection, so a thread blocked in `next` returns.
    pub fn stream(&self) -> &Stream {
        &self.stream
    }
}

/// What arrives on a watch connection.
pub enum Notice {
    /// The answer to `watch`.
    Level(Vec<Readable>),
    /// A process has output, or has exited.
    Readable(Readable),
}
