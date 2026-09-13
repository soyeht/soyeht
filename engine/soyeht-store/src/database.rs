use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{fmt, fs, io, path::Path, time::Duration};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 15;

#[derive(Debug)]
pub enum Error {
    Sql(rusqlite::Error),
    DiskFull,
    Json(serde_json::Error),
    Io(io::Error),
    FutureVersion(u32),
    IdempotencyConflict,
    CursorTooOld,
    DatabasePathIsSymlink,
    InvalidEngineGeneration,
    InvalidIdempotencyKey,
    EventParametersNotObject,
    CursorAhead,
    NegativeEventId,
    NegativeEventCursor,
    PairingRequestLimit,
    ApprovedPairTokenMissing,
    InvalidQueuedPrompt,
    PromptQueueFull,
    AttentionItemTooLarge,
    AttentionPaneMismatch,
    AgentRuntimeTooLarge,
    StaleAgentGeneration,
    NewOutboxHasProgress,
    IncompleteOutboxRecord,
    InvalidOutboxEnvelope,
    AgentOutboxFull,
    InvalidPartialWriteReceipt,
    NativeWriteAttemptsExhausted,
    NativeMessageIncomplete,
    InvalidNativeRequestIdentity,
    PublicRequestTooLarge,
    PendingRequestsTooLarge,
    NativeRequestNotFound,
    NativeEffectNotFound,
    NativeEffectNotPending,
    NativeEffectAlreadyStarted,
    InvalidTranscriptSequence,
    TranscriptMessageTooLarge,
    TranscriptMutation,
    StaleTranscriptReplay,
    InvalidTranscriptPageLimit,
    TranscriptPageTooLarge,
    InvalidSegmentEntry,
    InvalidSegmentName,
    InvalidSegmentHeader,
    SegmentHole,
    InvalidTerminalRecord,
    SegmentIndexMissing,
    SegmentRetentionLost,
    CursorOutOfRange,
    TerminalRecordTooLarge,
    MalformedRecordOffset,
    MalformedRecordLength,
    InvalidRecordLayout,
    TerminalOffsetOverflow,
    SegmentChecksumMismatch,
    MalformedGap,
    InvalidRecordInterval,
    InvalidTaskLimit,
    InvalidTaskNumber,
    InvalidTaskVersion,
    TaskVersionConflict,
    TaskDependencyCycle,
    SubitemNotFound,
    AttemptNotFound,
    TaskReservationConflict,
    InvalidReviewVersion,
    ReviewVersionConflict,
    InvalidUsageReading,
    InvalidEditorFile,
    InvalidFileHistoryLimit,
    FileVersionNotFound,
    InvalidFileReplacement,
    FileReplacementBusy,
    FileReplacementIncomplete,
    InvalidLanguageServerCheckpoint,
    InvalidEditorAgentReview,
    TestFixture(&'static str),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(e) => write!(f, "database: {e}"),
            Self::DiskFull => f.write_str("storage is full"),
            Self::Json(e) => write!(f, "JSON: {e}"),
            Self::Io(e) => write!(f, "storage: {e}"),
            Self::FutureVersion(v) => write!(f, "database schema {v} is newer than this engine"),
            Self::IdempotencyConflict => f.write_str("idempotency conflict"),
            Self::CursorTooOld => f.write_str("cursor too old"),
            Self::DatabasePathIsSymlink => f.write_str("database path is a symlink"),
            Self::InvalidEngineGeneration => f.write_str("invalid engine generation"),
            Self::InvalidIdempotencyKey => f.write_str("invalid idempotency scope or key"),
            Self::EventParametersNotObject => f.write_str("event parameters must be an object"),
            Self::CursorAhead => f.write_str("cursor is ahead of engine"),
            Self::NegativeEventId => f.write_str("negative event id"),
            Self::NegativeEventCursor => f.write_str("negative event cursor"),
            Self::PairingRequestLimit => f.write_str("pairing request limit reached"),
            Self::ApprovedPairTokenMissing => f.write_str("approved pair request has no token"),
            Self::InvalidQueuedPrompt => f.write_str("invalid queued prompt"),
            Self::PromptQueueFull => f.write_str("prompt queue is full"),
            Self::AttentionItemTooLarge => f.write_str("attention item exceeds its limit"),
            Self::AttentionPaneMismatch => {
                f.write_str("attention pane belongs to another workspace")
            }
            Self::AgentRuntimeTooLarge => f.write_str("agent runtime exceeds its limit"),
            Self::StaleAgentGeneration => f.write_str("stale agent generation"),
            Self::NewOutboxHasProgress => f.write_str("new outbox operation has delivery progress"),
            Self::IncompleteOutboxRecord => f.write_str("outbox needs one complete ACP line"),
            Self::InvalidOutboxEnvelope => f.write_str("outbox needs a JSON-RPC envelope"),
            Self::AgentOutboxFull => f.write_str("agent outbox is full"),
            Self::InvalidPartialWriteReceipt => f.write_str("invalid native partial-write receipt"),
            Self::NativeWriteAttemptsExhausted => f.write_str("native write attempts exhausted"),
            Self::NativeMessageIncomplete => f.write_str("native message is not fully delivered"),
            Self::InvalidNativeRequestIdentity => f.write_str("invalid native request identity"),
            Self::PublicRequestTooLarge => {
                f.write_str("public request exceeds the snapshot item limit")
            }
            Self::PendingRequestsTooLarge => {
                f.write_str("pending native requests exceed their limit")
            }
            Self::NativeRequestNotFound => f.write_str("native request not found"),
            Self::NativeEffectNotFound => f.write_str("native effect not found"),
            Self::NativeEffectNotPending => f.write_str("request is not a pending native effect"),
            Self::NativeEffectAlreadyStarted => f.write_str("native effect already started"),
            Self::InvalidTranscriptSequence => f.write_str("invalid transcript sequence"),
            Self::TranscriptMessageTooLarge => {
                f.write_str("transcript message exceeds its byte limit")
            }
            Self::TranscriptMutation => {
                f.write_str("transcript identity or completed content changed")
            }
            Self::StaleTranscriptReplay => f.write_str("replay belongs to a previous generation"),
            Self::InvalidTranscriptPageLimit => f.write_str("invalid transcript page limit"),
            Self::TranscriptPageTooLarge => f.write_str("transcript message exceeds page bounds"),
            Self::InvalidSegmentEntry => f.write_str("invalid segment filename or type"),
            Self::InvalidSegmentName => f.write_str("invalid segment filename"),
            Self::InvalidSegmentHeader => f.write_str("invalid segment header or size"),
            Self::SegmentHole => f.write_str("unmarked hole in terminal segments"),
            Self::InvalidTerminalRecord => f.write_str("noncontiguous or empty terminal record"),
            Self::SegmentIndexMissing => f.write_str("terminal segment was not created"),
            Self::SegmentRetentionLost => {
                f.write_str("terminal segment disappeared during retention")
            }
            Self::CursorOutOfRange => f.write_str("terminal cursor outside retained range"),
            Self::TerminalRecordTooLarge => f.write_str("terminal record too large"),
            Self::MalformedRecordOffset => f.write_str("terminal record offset is malformed"),
            Self::MalformedRecordLength => f.write_str("terminal record length is malformed"),
            Self::InvalidRecordLayout => f.write_str("invalid terminal record length or type"),
            Self::TerminalOffsetOverflow => f.write_str("terminal offset overflow"),
            Self::SegmentChecksumMismatch => f.write_str("terminal record checksum mismatch"),
            Self::MalformedGap => f.write_str("terminal gap length is malformed"),
            Self::InvalidRecordInterval => f.write_str("invalid terminal record interval"),
            Self::InvalidTaskLimit => f.write_str("invalid task page limit"),
            Self::InvalidTaskNumber => f.write_str("invalid task number"),
            Self::InvalidTaskVersion => f.write_str("invalid task version"),
            Self::TaskVersionConflict => f.write_str("task version conflict"),
            Self::TaskDependencyCycle => f.write_str("task dependency cycle"),
            Self::SubitemNotFound => f.write_str("task subitem not found"),
            Self::AttemptNotFound => f.write_str("task attempt not found"),
            Self::TaskReservationConflict => f.write_str("task reservation conflict"),
            Self::InvalidReviewVersion => f.write_str("invalid task review version"),
            Self::ReviewVersionConflict => f.write_str("task review version conflict"),
            Self::InvalidUsageReading => f.write_str("invalid stored usage reading"),
            Self::InvalidEditorFile => f.write_str("invalid stored editor file"),
            Self::InvalidFileHistoryLimit => f.write_str("invalid file history page limit"),
            Self::FileVersionNotFound => f.write_str("file version not found"),
            Self::InvalidFileReplacement => f.write_str("invalid file replacement"),
            Self::FileReplacementBusy => f.write_str("workspace already has a file replacement"),
            Self::FileReplacementIncomplete => f.write_str("file replacement is incomplete"),
            Self::InvalidLanguageServerCheckpoint => {
                f.write_str("invalid language server checkpoint")
            }
            Self::InvalidEditorAgentReview => f.write_str("invalid editor agent review"),
            Self::TestFixture(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        if matches!(
            e,
            rusqlite::Error::SqliteFailure(code, _)
                if code.code == rusqlite::ErrorCode::DiskFull
        ) {
            Self::DiskFull
        } else {
            Self::Sql(e)
        }
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub method: String,
    pub params: Value,
}

pub struct Mutation {
    pub result: Value,
    pub events: Vec<Event>,
    pub repeated: bool,
}

pub struct Snapshot {
    pub cursor: u64,
    pub sections: Vec<(String, Value)>,
}

pub struct Store {
    pub(crate) connection: Connection,
    pub db_id: String,
}

/// The only database write capability exposed to engine mutations.
///
/// Its driver transaction stays private to this crate. Domain modules add
/// named operations to this type so callers cannot execute arbitrary SQL.
pub struct Change<'connection> {
    pub(crate) transaction: Transaction<'connection>,
}

impl Change<'_> {
    pub(crate) fn connection(&self) -> &Connection {
        &self.transaction
    }
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory)?;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        let path = directory.join("soyeht.db");
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::DatabasePathIsSymlink);
        }
        let existed = path.exists();
        let mut connection = Connection::open(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::FutureVersion(version));
        }
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA wal_autocheckpoint=1000;")?;
        if version < SCHEMA_VERSION {
            if existed {
                let backups = directory.join("backups");
                fs::create_dir_all(&backups)?;
                fs::set_permissions(&backups, fs::Permissions::from_mode(0o700))?;
                let backup =
                    backups.join(format!("before-v{SCHEMA_VERSION}-{}.db", Uuid::new_v4()));
                connection.backup("main", &backup, None)?;
                fs::set_permissions(&backup, fs::Permissions::from_mode(0o600))?;
                fs::File::open(backup)?.sync_all()?;
                fs::File::open(backups)?.sync_all()?;
            }
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if version < 1 {
                tx.execute_batch(include_str!("../migrations/001-initial.sql"))?;
                tx.execute(
                    "INSERT INTO metadata(key,value) VALUES ('db_id',?1),('engine_generation','0')",
                    [Uuid::new_v4().to_string()],
                )?;
            }
            if version < 2 {
                tx.execute_batch(include_str!("../migrations/002-client-state.sql"))?;
            }
            if version < 3 {
                crate::english_values::migrate(&tx)?;
            }
            if version < 4 {
                tx.execute_batch(include_str!("../migrations/004-agent-sessions.sql"))?;
            }
            if version < 5 {
                tx.execute_batch(include_str!("../migrations/005-native-replay.sql"))?;
            }
            if version < 6 {
                tx.execute_batch(include_str!("../migrations/006-agent-auth-snapshot.sql"))?;
            }
            if version < 7 {
                tx.execute_batch(include_str!("../migrations/007-workspace-tasks.sql"))?;
            }
            if version < 8 {
                tx.execute_batch(include_str!("../migrations/008-task-reviews.sql"))?;
            }
            if version < 9 {
                tx.execute_batch(include_str!("../migrations/009-task-block-retries.sql"))?;
            }
            if version < 10 {
                tx.execute_batch(include_str!("../migrations/010-agent-pane-role.sql"))?;
            }
            if version < 11 {
                tx.execute_batch(include_str!("../migrations/011-usage-readings.sql"))?;
            }
            if version < 12 {
                tx.execute_batch(include_str!("../migrations/012-editor-files.sql"))?;
            }
            if version < 13 {
                tx.execute_batch(include_str!(
                    "../migrations/013-editor-project-replacements.sql"
                ))?;
            }
            if version < 14 {
                tx.execute_batch(include_str!("../migrations/014-language-servers.sql"))?;
            }
            if version < 15 {
                tx.execute_batch(include_str!("../migrations/015-editor-agent-reviews.sql"))?;
            }
            tx.commit()?;
        }
        let db_id =
            connection.query_row("SELECT value FROM metadata WHERE key='db_id'", [], |row| {
                row.get(0)
            })?;
        Ok(Self { connection, db_id })
    }

    /// This connection belongs only to the engine. All writes use its transaction.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn next_engine_generation(&mut self) -> Result<u64> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: String = tx.query_row(
            "SELECT value FROM metadata WHERE key='engine_generation'",
            [],
            |row| row.get(0),
        )?;
        let next = prior
            .parse::<u64>()
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(Error::InvalidEngineGeneration)?;
        tx.execute(
            "UPDATE metadata SET value=?1 WHERE key='engine_generation'",
            [next.to_string()],
        )?;
        tx.commit()?;
        Ok(next)
    }

    pub fn mutate<F>(
        &mut self,
        scope: &str,
        key: &str,
        fingerprint: &str,
        apply: F,
    ) -> Result<Mutation>
    where
        F: FnOnce(&Change<'_>) -> Result<(Value, Vec<(String, Value)>)>,
    {
        self.mutate_as(scope, key, fingerprint, None, apply)
    }

    pub fn mutate_as<F>(
        &mut self,
        scope: &str,
        key: &str,
        fingerprint: &str,
        actor: Option<&Value>,
        apply: F,
    ) -> Result<Mutation>
    where
        F: FnOnce(&Change<'_>) -> Result<(Value, Vec<(String, Value)>)>,
    {
        if scope.is_empty() || key.is_empty() || key.len() > 128 {
            return Err(Error::InvalidIdempotencyKey);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let change = Change { transaction };
        let previous: Option<(String, String)> = change
            .transaction
            .query_row(
                "SELECT fingerprint,result FROM idempotency WHERE scope=?1 AND key=?2",
                params![scope, key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((prior, result)) = previous {
            if prior != fingerprint {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(Mutation {
                result: serde_json::from_str(&result)?,
                events: Vec::new(),
                repeated: true,
            });
        }
        let (result, pending) = apply(&change)?;
        let mut events = Vec::new();
        let at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let now = Utc::now().timestamp();
        for (method, mut payload) in pending {
            let object = payload
                .as_object_mut()
                .ok_or(Error::EventParametersNotObject)?;
            change.transaction.execute(
                "INSERT INTO events(method,params,created_unix) VALUES (?1,'{}',?2)",
                params![method, now],
            )?;
            let id = change.transaction.last_insert_rowid() as u64;
            object.insert("eventId".into(), json!(id.to_string()));
            object.insert("at".into(), json!(at));
            if let Some(actor) = actor {
                object.entry("actor").or_insert_with(|| actor.clone());
            }
            change.transaction.execute(
                "UPDATE events SET params=?1 WHERE id=?2",
                params![serde_json::to_string(&payload)?, id as i64],
            )?;
            events.push(Event {
                id,
                method,
                params: payload,
            });
        }
        change.transaction.execute("INSERT INTO idempotency(scope,key,fingerprint,result,created_unix,actor) VALUES (?1,?2,?3,?4,?5,?6)", params![scope,key,fingerprint,serde_json::to_string(&result)?,now,actor.map(serde_json::to_string).transpose()?])?;
        change.transaction.commit()?;
        Ok(Mutation {
            result,
            events,
            repeated: false,
        })
    }

    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let tx = self.connection.transaction()?;
        let cursor = cursor(&tx)?;
        let workspaces = rows_json(
            &tx,
            "SELECT payload FROM workspaces WHERE closed=0 ORDER BY id",
        )?;
        let panes = rows_json(&tx, "SELECT payload FROM panes WHERE closed=0 ORDER BY id")?;
        let devices = rows_json(
            &tx,
            "SELECT payload FROM devices WHERE revoked=0 ORDER BY id",
        )?;
        let requests = rows_json(
            &tx,
            "SELECT json_object('kind',CASE method WHEN 'session/request_permission' THEN 'permission' ELSE 'elicitation' END,'request',json(public)) FROM agent_requests WHERE result IS NULL AND method IN ('session/request_permission','elicitation/create') AND pane_id IN (SELECT id FROM panes WHERE closed=0) ORDER BY request_id",
        )?;
        let tasks = rows_json(
            &tx,
            "SELECT payload FROM tasks ORDER BY workspace_id,priority_rank,ordinal",
        )?;
        let loops = rows_json(
            &tx,
            "SELECT json_object('paneId',pane_id,'config',json(payload)) FROM task_loops ORDER BY pane_id",
        )?;
        let files = crate::editor::snapshot_files(&tx)?;
        let sections = vec![
            ("workspaces".into(), json!(workspaces)),
            ("panes".into(), json!(panes)),
            ("devices".into(), json!(devices)),
            ("requests".into(), json!(requests)),
            ("tasks".into(), json!(tasks)),
            ("loops".into(), json!(loops)),
            ("files".into(), json!(files)),
            (
                "attention".into(),
                json!(rows_json(
                    &tx,
                    "SELECT a.payload FROM agent_attention a JOIN panes p ON p.id=a.pane_id WHERE p.closed=0 ORDER BY a.ordinal"
                )?),
            ),
        ];
        tx.commit()?;
        Ok(Snapshot { cursor, sections })
    }

    pub fn replay(&mut self, since: u64) -> Result<(u64, Vec<Event>)> {
        let tx = self.connection.transaction()?;
        let through = cursor(&tx)?;
        if since > through {
            return Err(Error::CursorAhead);
        }
        let first: Option<i64> =
            tx.query_row("SELECT MIN(id) FROM events", [], |row| row.get(0))?;
        if since
            < first
                .map(|n| n as u64)
                .unwrap_or(through.saturating_add(1))
                .saturating_sub(1)
        {
            return Err(Error::CursorTooOld);
        }
        let mut statement =
            tx.prepare("SELECT id,method,params FROM events WHERE id>?1 AND id<=?2 ORDER BY id")?;
        let rows = statement.query_map(params![since as i64, through as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (id, method, raw) = row?;
            events.push(Event {
                id: u64::try_from(id).map_err(|_| Error::NegativeEventId)?,
                method,
                params: serde_json::from_str(&raw)?,
            });
        }
        drop(statement);
        tx.commit()?;
        Ok((through, events))
    }

    pub fn prune(&mut self) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = cursor(&tx)?;
        tx.execute(
            "DELETE FROM events WHERE id<=?1 OR created_unix<?2",
            params![
                current.saturating_sub(10_000) as i64,
                Utc::now().timestamp() - 7 * 86400
            ],
        )?;
        tx.execute(
            "DELETE FROM idempotency WHERE created_unix<?1",
            [Utc::now().timestamp() - 86400],
        )?;
        tx.execute(
            "DELETE FROM pair_requests WHERE expires_unix<=?1",
            [Utc::now().timestamp()],
        )?;
        tx.execute(
            "DELETE FROM pair_windows WHERE expires_unix<=?1",
            [Utc::now().timestamp()],
        )?;
        tx.commit()?;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
        Ok(())
    }
}

fn cursor(connection: &Connection) -> Result<u64> {
    let id: i64 = connection
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='events'",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    u64::try_from(id).map_err(|_| Error::NegativeEventCursor)
}

fn rows_json(connection: &Connection, sql: &str) -> Result<Vec<Value>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
