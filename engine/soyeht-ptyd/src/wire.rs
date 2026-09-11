use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessKind {
    Pty,
    PipesAcp,
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
    },
    List,
    Spawn(Spawn),
    Close {
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
    },
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
