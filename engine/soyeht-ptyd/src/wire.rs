use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The version of the supervisor protocol. A change of it counts as a change of the supervisor for
/// an update (ADR 0122), and the Windows installer compares it (`--protocol-version`).
///
/// 6: a connection first proves that it can read the supervisor's secret (ADR 0160), so an engine
/// of this build cannot talk to a supervisor of version 5 or the other way round.
pub const VERSION: u32 = 6;

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
/// The supervisor answers `Foreground` (ADR 0165). It is a feature and not a version, so a running
/// supervisor of this protocol that does not list it keeps working after an engine update.
pub const FEATURE_FOREGROUND: &str = "foreground";
/// The supervisor answers `CloseMany` (ADR 0171). An engine that does not find it closes each process
/// with a `Close` of its own.
pub const FEATURE_CLOSE_MANY: &str = "close-many";

/// A supervisor that lists this accepts `SOYEHT_PANE_BINDING` in the environment of a terminal
/// (ADR 0165). An older one refuses the variable as one the engine may not set, so the engine
/// leaves it out of a start it sends to a supervisor that does not list the feature: the
/// terminal runs, and is not attached to a Soyeht pane.
pub const FEATURE_PANE_BINDING: &str = "pane-binding";

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
    /// Closes these processes together (at most [`MAX_CLOSE_MANY`]): the answer comes when all of
    /// them are gone.
    CloseMany {
        instance_ids: Vec<String>,
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
    /// What each of these terminals runs in its foreground (at most [`MAX_FOREGROUND`]).
    Foreground {
        instance_ids: Vec<String>,
    },
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
    /// The answer to `Foreground`, for the processes that are terminals the supervisor holds.
    Foregrounds(Vec<Foreground>),
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

/// How many processes one `CloseMany` may name.
pub const MAX_CLOSE_MANY: usize = 256;

/// How many terminals one `Foreground` may ask about.
pub const MAX_FOREGROUND: usize = 256;

/// The most bytes of file names and arguments one `Foregrounds` answer holds. An eighth of the
/// largest JSON frame: a string can grow to six times its length when it is escaped, so the
/// answer stays inside the frame whatever the terminals run. A terminal that does not fit is
/// left out of the answer, and one that fits in part is marked `truncated`.
pub const MAX_FOREGROUND_BYTES: usize = soyeht_protocol::limits::JSON_BYTES / 8;

/// What one terminal runs in its foreground: on Unix the leader of the foreground process group,
/// on Windows the processes of the terminal's job, a process before what it started. No programs
/// when the terminal has none left to read (it ended, or the group's leader did).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Foreground {
    pub instance_id: String,
    pub programs: Vec<Program>,
    /// Whether programs were left out for want of room in the answer ([`MAX_FOREGROUND_BYTES`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// A process in a terminal's foreground.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Program {
    pub pid: u32,
    /// The parent, when it is another program of the same list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_pid: Option<u32>,
    /// The file it runs, when the system tells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// Its `argv`, `argv[0]` first, bounded in number and length.
    #[serde(default)]
    pub arguments: Vec<String>,
}

impl From<soyeht_platform::process::inspect::Program> for Program {
    fn from(program: soyeht_platform::process::inspect::Program) -> Self {
        Self {
            pid: program.pid,
            parent_pid: program.parent,
            executable: program.executable,
            arguments: program.arguments,
        }
    }
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
