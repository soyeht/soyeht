use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{fmt, fs, io, path::Path, time::Duration};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub enum Error {
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    Io(io::Error),
    FutureVersion(u32),
    IdempotencyConflict,
    CursorTooOld,
    Invalid(&'static str),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(e) => write!(f, "database: {e}"),
            Self::Json(e) => write!(f, "JSON: {e}"),
            Self::Io(e) => write!(f, "storage: {e}"),
            Self::FutureVersion(v) => write!(f, "database schema {v} is newer than this engine"),
            Self::IdempotencyConflict => f.write_str("idempotency conflict"),
            Self::CursorTooOld => f.write_str("cursor too old"),
            Self::Invalid(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
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
    connection: Connection,
    pub db_id: String,
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory)?;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        let path = directory.join("soyeht.db");
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Invalid("database path is a symlink"));
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
            tx.execute_batch(include_str!("../migrations/001-initial.sql"))?;
            tx.execute(
                "INSERT INTO metadata(key,value) VALUES ('db_id',?1),('engine_generation','0')",
                [Uuid::new_v4().to_string()],
            )?;
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
            .ok_or(Error::Invalid("invalid engine generation"))?;
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
        change: F,
    ) -> Result<Mutation>
    where
        F: FnOnce(&Transaction<'_>) -> Result<(Value, Vec<(String, Value)>)>,
    {
        if scope.is_empty() || key.is_empty() || key.len() > 128 {
            return Err(Error::Invalid("invalid idempotency scope or key"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<(String, String)> = tx
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
        let (result, pending) = change(&tx)?;
        let mut events = Vec::new();
        let at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let now = Utc::now().timestamp();
        for (method, mut payload) in pending {
            let object = payload
                .as_object_mut()
                .ok_or(Error::Invalid("event parameters must be an object"))?;
            tx.execute(
                "INSERT INTO events(method,params,created_unix) VALUES (?1,'{}',?2)",
                params![method, now],
            )?;
            let id = tx.last_insert_rowid() as u64;
            object.insert("eventId".into(), json!(id.to_string()));
            object.insert("at".into(), json!(at));
            tx.execute(
                "UPDATE events SET params=?1 WHERE id=?2",
                params![serde_json::to_string(&payload)?, id as i64],
            )?;
            events.push(Event {
                id,
                method,
                params: payload,
            });
        }
        tx.execute("INSERT INTO idempotency(scope,key,fingerprint,result,created_unix) VALUES (?1,?2,?3,?4,?5)", params![scope,key,fingerprint,serde_json::to_string(&result)?,now])?;
        tx.commit()?;
        Ok(Mutation {
            result,
            events,
            repeated: false,
        })
    }

    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let tx = self.connection.transaction()?;
        let cursor = cursor(&tx)?;
        let workspaces = rows_json(&tx, "SELECT payload FROM workspaces ORDER BY id")?;
        let panes = rows_json(&tx, "SELECT payload FROM panes WHERE closed=0 ORDER BY id")?;
        let devices = rows_json(
            &tx,
            "SELECT payload FROM devices WHERE revoked=0 ORDER BY id",
        )?;
        let sections = vec![
            ("workspaces".into(), json!(workspaces)),
            ("panes".into(), json!(panes)),
            ("devices".into(), json!(devices)),
        ];
        tx.commit()?;
        Ok(Snapshot { cursor, sections })
    }

    pub fn replay(&mut self, since: u64) -> Result<(u64, Vec<Event>)> {
        let tx = self.connection.transaction()?;
        let through = cursor(&tx)?;
        if since > through {
            return Err(Error::Invalid("cursor is ahead of engine"));
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
                id: u64::try_from(id).map_err(|_| Error::Invalid("negative event id"))?,
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
    u64::try_from(id).map_err(|_| Error::Invalid("negative event cursor"))
}

fn rows_json(connection: &Connection, sql: &str) -> Result<Vec<Value>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
