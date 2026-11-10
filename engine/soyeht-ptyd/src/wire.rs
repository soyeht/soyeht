use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: u32 = 5;

/// How far past the generation it has recorded the supervisor lets a `hello` raise it (ADR 0160).
///
/// An engine takes the next number of its database each time it starts, whether or not it then
/// reaches the supervisor, so the numbers it presents can only run ahead of the recorded one by
/// the starts that never got through: a crash loop before the first connection, or a supervisor
/// that was down. The step is far more than a person's restarts and far less than a number that
/// would shut every real engine out, which is what a `hello` of `u64::MAX` did. The first engine
/// to meet a supervisor with an empty registry (generation 0) is not held to it: there is
/// nothing recorded to be ahead of.
pub const MAX_GENERATION_STEP: u64 = 16;

/// What a connection is for. The default is the request connection, the only one older engines and
/// supervisors know, so a hello without a role is the one of today.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Request,
    /// A connection that only receives notices: `Watch` once, then `Readable` as processes have
    /// output. It is not the engine's request connection and does not evict it.
    Watch,
}

impl Role {
    fn is_request(&self) -> bool {
        *self == Self::Request
    }
}

/// A feature a supervisor lists in its welcome. An engine that does not find it does not use it.
pub const FEATURE_WATCH: &str = "watch";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessKind {
    Pty,
    PipesAcp,
    PipesLsp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Spawn {
    pub instance_key: String,
    pub generation: u32,
    pub kind: ProcessKind,
    pub cwd: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub instance_id: String,
    pub supervisor_id: String,
    pub spawn: Spawn,
    pub pid: u32,
    pub closed: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    Hello {
        version: u32,
        engine_generation: u64,
        /// Left out when it is the default, so a request hello is byte for byte the one an older
        /// supervisor reads.
        #[serde(default, skip_serializing_if = "Role::is_request")]
        role: Role,
    },
    /// On a watch connection: the processes the engine has read to, so the supervisor answers
    /// with those that already have more (level before edge).
    Watch {
        instances: Vec<Watched>,
    },
    List,
    Spawn(Spawn),
    Close {
        instance_id: String,
    },
    Kill {
        instance_id: String,
    },
    Input {
        instance_id: String,
        bytes: String,
        operation_key: String,
        fingerprint: String,
    },
    ConfirmInput {
        operation_key: String,
    },
    PendingInputs,
    PendingRequests {
        instance_id: String,
    },
    PendingRequestChunk {
        instance_id: String,
        request_key: u64,
        offset: usize,
    },
    Resize {
        instance_id: String,
        cols: u16,
        rows: u16,
    },
    Drain {
        instance_id: String,
        from_seq: u64,
    },
    Ack {
        instance_id: String,
        seq: u64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum Response {
    Welcome {
        version: u32,
        supervisor_id: String,
        supervisor_generation: u32,
        /// What this supervisor can do beyond the first protocol. An older one sends none.
        #[serde(default)]
        features: Vec<String>,
    },
    /// The answer to `Watch`: the processes that have output past what the engine said it had
    /// read, or have exited.
    Watching(Vec<Readable>),
    /// Pushed on a watch connection: a process has output the engine has not read.
    Readable(Readable),
    Instances(Vec<Instance>),
    Instance(Instance),
    Output(Output),
    Accepted {
        bytes: usize,
    },
    InputReceipts(Vec<String>),
    PendingRequests {
        items: Vec<PendingRequest>,
        recovery_gap: bool,
    },
    PendingRequestChunk(PendingRequestChunk),
    Ok,
    Error {
        kind: String,
        message: String,
    },
}

/// A process the engine has read to a point.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Watched {
    pub instance_id: String,
    pub from_seq: u64,
}

/// A process with output to read, or that has exited and has no more.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Readable {
    pub instance_id: String,
    /// The end of what the supervisor holds for it.
    pub end_seq: u64,
    pub exited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Gap {
    pub from_seq: u64,
    pub to_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    pub request_key: u64,
    pub from_seq: u64,
    pub to_seq: u64,
    pub bytes: usize,
    pub method: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PendingRequestChunk {
    pub request_key: u64,
    pub offset: usize,
    pub next_offset: usize,
    pub total_bytes: usize,
    pub bytes: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub from_seq: u64,
    pub next_seq: u64,
    pub end_seq: u64,
    pub bytes: String,
    pub gap: Option<Gap>,
    pub exited: bool,
}
