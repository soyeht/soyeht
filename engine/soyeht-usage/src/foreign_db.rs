//! Read-only access to other applications' SQLite databases: the Cursor app's state database,
//! the browsers' cookie stores (CodexBar `CursorAppAuth`, its cookie import) and OpenCode's
//! database (its credential table and the usage its sessions recorded).
//!
//! This is the only module outside `soyeht-store` that opens SQLite, a documented exception in
//! the hardening gate (ADR 0077). Every database is opened read-only, with `immutable=1` where
//! the owning application may hold it open, so no lock is taken and nothing is ever written.

use crate::host::{Cookie, LocalUsage, LocalUsageRead, Secret, SecretRead, StoredCredential};
use rusqlite::{Connection, OpenFlags, OptionalExtension, types::ValueRef};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

/// How long a state-database read waits for the owning application's write lock.
const BUSY_WAIT: Duration = Duration::from_millis(250);

/// The longest a read of another application's database may keep its caller: the usage refresh
/// joins every provider before it commits, so one stalled read would hold back the others.
const READ_DEADLINE: Duration = Duration::from_secs(5);

/// How many virtual-machine instructions SQLite runs between two looks at the deadline.
const DEADLINE_CHECK_EVERY: i32 = 1_000;

/// Reads that may be stuck in the file system at once. A stalled read cannot be cancelled (SQLite
/// looks at its deadline between instructions, not inside a blocked system call), so its thread
/// ends only when the file system answers; past this count a new read is refused instead of
/// piling up another thread behind the stall.
const MAX_STUCK_READS: usize = 4;

static READS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Counts a read as in flight until it ends, on whichever thread it ends.
struct InFlight(&'static AtomicUsize);

impl InFlight {
    fn enter(count: &'static AtomicUsize) -> Option<Self> {
        (count.fetch_add(1, Ordering::AcqRel) < MAX_STUCK_READS)
            .then_some(Self(count))
            .or_else(|| {
                count.fetch_sub(1, Ordering::AcqRel);
                None
            })
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Runs `read` on its own thread and gives up on it after `limit`, so a file system that stops
/// answering costs the caller `limit` and no more. The read gets the instant its queries must
/// end by (`open_live` hands it to SQLite) so a slow query ends itself on that thread.
fn bounded<T: Send + 'static>(
    limit: Duration,
    count: &'static AtomicUsize,
    read: impl FnOnce(Instant) -> Result<T, Unreadable> + Send + 'static,
) -> Result<T, Unreadable> {
    let Some(slot) = InFlight::enter(count) else {
        return Err(Unreadable("stalled".to_owned()));
    };
    let deadline = Instant::now() + limit;
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("foreign-db-read".to_owned())
        .spawn(move || {
            let outcome = read(deadline);
            drop(slot);
            // ignored: the caller stopped waiting, and the outcome is of no use to anyone
            let _ = sender.send(outcome);
        })
        .map_err(|_| Unreadable("other".to_owned()))?;
    receiver
        .recv_timeout(limit)
        .unwrap_or_else(|_| Err(Unreadable("timeout".to_owned())))
}

/// A Chromium cookie as stored: still encrypted, with the database's schema version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedCookie {
    pub name: String,
    pub encrypted: Vec<u8>,
    pub version: i64,
}

fn flags() -> OpenFlags {
    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX
}

/// Opens a database another application may hold open, without taking its lock.
fn open_immutable(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(format!("file:{}?immutable=1", path.display()), flags())
}

/// Opens a database another application may hold open for writing, read-only. A database in
/// write-ahead mode is read with its log, so a recent write is seen; one that cannot be opened
/// that way (a read-only directory without the shared-memory file) is opened `immutable=1`.
/// No query outlives `deadline`: SQLite interrupts it and the read reports `OperationInterrupted`.
fn open_live(path: &Path, deadline: Instant) -> rusqlite::Result<Connection> {
    let connection =
        Connection::open_with_flags(path, flags()).or_else(|_| open_immutable(path))?;
    // ignored: a connection that cannot set its wait fails fast, and the read reports it
    let _ = connection.busy_timeout(BUSY_WAIT);
    connection.progress_handler(
        DEADLINE_CHECK_EVERY,
        Some(move || Instant::now() >= deadline),
    )?;
    Ok(connection)
}

fn has_table(connection: &Connection, name: &str) -> rusqlite::Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |_| Ok(()),
        )
        .optional()
        .map(|found| found.is_some())
}

/// Why a state database could not be read: SQLite's result code, never its message, which can
/// carry the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable(pub String);

impl Unreadable {
    fn of(error: &rusqlite::Error) -> Self {
        Self(match error {
            rusqlite::Error::SqliteFailure(failure, _) => format!("{:?}", failure.code),
            _ => "other".to_owned(),
        })
    }
}

/// A value from the `ItemTable` of an Electron state database (Cursor's `state.vscdb`). A
/// database that cannot be opened or read is an `Unreadable`, which the caller turns into
/// `Denied` after logging its kind.
pub fn item_table_value(path: &Path, key: &str) -> Result<SecretRead, Unreadable> {
    if !path.is_file() {
        return Ok(SecretRead::Missing);
    }
    // CodexBar opens it read-only and falls back to `immutable=1` when that fails; the
    // fallback's error is the one reported.
    let connection = Connection::open_with_flags(path, flags())
        .or_else(|_| open_immutable(path))
        .map_err(|error| Unreadable::of(&error))?;
    // ignored: a connection that cannot set its wait fails fast, and the read reports it
    let _ = connection.busy_timeout(BUSY_WAIT);
    let value = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
            [key],
            |row| Ok(text_value(row.get_ref(0)?)),
        )
        .optional();
    match value {
        Ok(Some(Some(text))) => Ok(SecretRead::Found(Secret::new(text))),
        Ok(_) => Ok(SecretRead::Missing),
        Err(error) => Err(Unreadable::of(&error)),
    }
}

/// The active credential of `integration` in OpenCode's `credential` table (the newest one when
/// no row is marked active). The value is the JSON text OpenCode stored; the caller reads the
/// secret out of it. The read ends after `READ_DEADLINE` as an `Unreadable`.
pub fn opencode_credential(path: &Path, integration: &str) -> Result<StoredCredential, Unreadable> {
    let (path, integration) = (path.to_path_buf(), integration.to_owned());
    bounded(READ_DEADLINE, &READS_IN_FLIGHT, move |deadline| {
        read_opencode_credential(&path, &integration, deadline)
    })
}

fn read_opencode_credential(
    path: &Path,
    integration: &str,
    deadline: Instant,
) -> Result<StoredCredential, Unreadable> {
    if !path.is_file() {
        return Ok(StoredCredential::NoTable);
    }
    let connection = open_live(path, deadline).map_err(|error| Unreadable::of(&error))?;
    let read = || -> rusqlite::Result<StoredCredential> {
        if !has_table(&connection, "credential")? {
            return Ok(StoredCredential::NoTable);
        }
        let rows: i64 =
            connection.query_row("SELECT count(*) FROM credential", [], |row| row.get(0))?;
        if rows == 0 {
            return Ok(StoredCredential::NoTable);
        }
        let value = connection
            .query_row(
                "SELECT value FROM credential WHERE integration_id = ?1 \
                 ORDER BY active DESC, time_created DESC, id DESC LIMIT 1",
                [integration],
                |row| Ok(text_value(row.get_ref(0)?)),
            )
            .optional()?;
        Ok(match value {
            Some(Some(text)) => StoredCredential::Found(Secret::new(text)),
            _ => StoredCredential::NotInTable,
        })
    };
    read().map_err(|error| Unreadable::of(&error))
}

/// The tokens and cost OpenCode recorded on assistant messages created at or after `since_ms`.
/// The sessions touched in the window are chosen first, so the large `message` table is read
/// through its session index and not scanned. A message whose `data` is not JSON is skipped, not
/// a reason to give up on the rest. The read ends after `READ_DEADLINE` as an `Unreadable`.
pub fn opencode_usage(
    path: &Path,
    since_ms: i64,
    provider: Option<&str>,
) -> Result<LocalUsageRead, Unreadable> {
    let (path, provider) = (path.to_path_buf(), provider.map(str::to_owned));
    bounded(READ_DEADLINE, &READS_IN_FLIGHT, move |deadline| {
        read_opencode_usage(&path, since_ms, provider.as_deref(), deadline)
    })
}

fn read_opencode_usage(
    path: &Path,
    since_ms: i64,
    provider: Option<&str>,
    deadline: Instant,
) -> Result<LocalUsageRead, Unreadable> {
    if !path.is_file() {
        return Ok(LocalUsageRead::NoDatabase);
    }
    let connection = open_live(path, deadline).map_err(|error| Unreadable::of(&error))?;
    let read = || -> rusqlite::Result<LocalUsageRead> {
        if !has_table(&connection, "message")? || !has_table(&connection, "session")? {
            return Ok(LocalUsageRead::NoHistory);
        }
        let (tokens, cost): (i64, f64) = connection.query_row(
            "SELECT \
               COALESCE(SUM(COALESCE(json_extract(data, '$.tokens.input'), 0) \
                 + COALESCE(json_extract(data, '$.tokens.output'), 0) \
                 + COALESCE(json_extract(data, '$.tokens.reasoning'), 0)), 0), \
               COALESCE(SUM(COALESCE(json_extract(data, '$.cost'), 0)), 0.0) \
             FROM message \
             WHERE session_id IN (SELECT id FROM session WHERE time_updated >= ?1) \
               AND time_created >= ?1 \
               AND json_valid(data) \
               AND json_extract(data, '$.role') = 'assistant' \
               AND (?2 IS NULL OR json_extract(data, '$.providerID') = ?2)",
            rusqlite::params![since_ms, provider],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(LocalUsageRead::Totals(LocalUsage {
            tokens: u64::try_from(tokens).unwrap_or(0),
            cost_usd: if cost.is_finite() { cost.max(0.0) } else { 0.0 },
        }))
    };
    read().map_err(|error| Unreadable::of(&error))
}

/// TEXT, or a BLOB holding UTF-8 or (ASCII with interleaved NULs) UTF-16LE.
fn text_value(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Text(bytes) => String::from_utf8(bytes.to_vec()).ok(),
        ValueRef::Blob(bytes)
            if bytes.len().is_multiple_of(2)
                && bytes.iter().skip(1).step_by(2).all(|b| *b == 0) =>
        {
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            String::from_utf16(&units).ok()
        }
        ValueRef::Blob(bytes) => String::from_utf8(bytes.to_vec()).ok(),
        _ => None,
    }
    .map(|text| text.trim().to_owned())
    .filter(|text| !text.is_empty())
}

/// The host patterns of a domain: exact, dotted and any subdomain.
fn host_patterns(domain: &str) -> [String; 3] {
    [
        domain.to_owned(),
        format!(".{domain}"),
        format!("%.{domain}"),
    ]
}

/// Every cookie of `domain` in a Chromium `Cookies` database.
pub fn chromium_cookies(database: &Path, domain: &str) -> Vec<EncryptedCookie> {
    let Ok(connection) = open_immutable(database) else {
        return Vec::new();
    };
    let version: i64 = connection
        .query_row("SELECT value FROM meta WHERE key='version'", [], |row| {
            row.get::<_, String>(0)
        })
        // ignored: a database with no readable version is treated as version 0
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let Ok(mut statement) = connection.prepare(
        "SELECT name, encrypted_value FROM cookies WHERE host_key=?1 OR host_key=?2 OR host_key LIKE ?3",
    ) else {
        return Vec::new();
    };
    statement
        .query_map(host_patterns(domain), |row| {
            Ok(EncryptedCookie {
                name: row.get(0)?,
                encrypted: row.get(1)?,
                version,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// Every cookie of `domain` in a Firefox `cookies.sqlite` database (stored in clear).
pub fn firefox_cookies(database: &Path, domain: &str) -> Vec<Cookie> {
    let Ok(connection) = open_immutable(database) else {
        return Vec::new();
    };
    let Ok(mut statement) = connection
        .prepare("SELECT name, value FROM moz_cookies WHERE host=?1 OR host=?2 OR host LIKE ?3")
    else {
        return Vec::new();
    };
    statement
        .query_map(host_patterns(domain), |row| {
            Ok(Cookie {
                name: row.get(0)?,
                value: Secret::new(row.get::<_, String>(1)?),
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn database(name: &str, schema: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("sn-foreign-{}-{name}", uuid::Uuid::new_v4()));
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(schema).unwrap();
        path
    }

    #[test]
    fn state_values_are_read_without_writing() {
        let path = database(
            "state.vscdb",
            "CREATE TABLE ItemTable (key TEXT UNIQUE, value BLOB);
             INSERT INTO ItemTable VALUES ('text', 'abc'), ('blob', x'61006200');",
        );
        let before = fs::read(&path).unwrap();
        assert_eq!(
            item_table_value(&path, "text"),
            Ok(SecretRead::Found(Secret::new("abc")))
        );
        assert_eq!(
            item_table_value(&path, "blob"),
            Ok(SecretRead::Found(Secret::new("ab")))
        );
        assert_eq!(item_table_value(&path, "absent"), Ok(SecretRead::Missing));
        assert_eq!(
            item_table_value(&path.with_extension("missing"), "text"),
            Ok(SecretRead::Missing)
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_database_is_unreadable_and_names_a_result_code_only() {
        let path = std::env::temp_dir().join(format!("sn-foreign-{}-bad", uuid::Uuid::new_v4()));
        fs::write(
            &path,
            b"this is not a sqlite database, only some text padding it out",
        )
        .unwrap();
        let Err(Unreadable(kind)) = item_table_value(&path, "text") else {
            panic!("a text file was read as a database");
        };
        assert!(!kind.is_empty() && !kind.contains('/'), "{kind}");
        fs::remove_file(path).unwrap();
    }

    const OPENCODE_SCHEMA: &str =
        "CREATE TABLE session (id TEXT PRIMARY KEY, time_updated INTEGER NOT NULL);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
           time_created INTEGER NOT NULL, data TEXT NOT NULL);";

    fn message(id: &str, session: &str, at: i64, data: &str) -> String {
        format!("INSERT INTO message VALUES ('{id}', '{session}', {at}, '{data}');")
    }

    #[test]
    fn opencode_usage_sums_assistant_messages_of_the_window_and_provider_without_writing() {
        let assistant = |provider: &str, input: i64, cost: f64| {
            format!(
                r#"{{"role":"assistant","providerID":"{provider}","cost":{cost},"tokens":{{"input":{input},"output":10,"reasoning":5,"cache":{{"read":999,"write":999}}}}}}"#
            )
        };
        let rows = [
            message("m1", "s1", 2_000, &assistant("zai", 100, 0.5)),
            message("m2", "s1", 3_000, &assistant("zai", 200, 0.25)),
            message("m3", "s1", 3_000, &assistant("openai", 7, 1.0)),
            // before the window, though its session was touched inside it
            message("m4", "s1", 500, &assistant("zai", 5_000, 9.0)),
            message("m5", "s1", 3_000, r#"{"role":"user"}"#),
            // a session not touched in the window
            message("m6", "s2", 3_000, &assistant("zai", 4_000, 9.0)),
            message(
                "m7",
                "s1",
                3_000,
                r#"{"role":"assistant","providerID":"zai"}"#,
            ),
        ]
        .concat();
        let path = database(
            "opencode.db",
            &format!(
                "{OPENCODE_SCHEMA}
                 INSERT INTO session VALUES ('s1', 3000), ('s2', 100);
                 {rows}"
            ),
        );
        let before = fs::read(&path).unwrap();
        // 100 + 10 + 5 and 200 + 10 + 5 of m1 and m2, and m7 records nothing.
        assert_eq!(
            opencode_usage(&path, 1_000, Some("zai")),
            Ok(LocalUsageRead::Totals(LocalUsage {
                tokens: 330,
                cost_usd: 0.75
            }))
        );
        assert_eq!(
            opencode_usage(&path, 1_000, None),
            Ok(LocalUsageRead::Totals(LocalUsage {
                tokens: 352,
                cost_usd: 1.75
            }))
        );
        assert_eq!(
            opencode_usage(&path, 1_000, Some("nobody")),
            Ok(LocalUsageRead::Totals(LocalUsage {
                tokens: 0,
                cost_usd: 0.0
            }))
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn opencode_usage_tells_a_missing_database_from_a_layout_it_does_not_know() {
        let path = std::env::temp_dir().join(format!("sn-foreign-{}-none", uuid::Uuid::new_v4()));
        assert_eq!(
            opencode_usage(&path, 0, None),
            Ok(LocalUsageRead::NoDatabase)
        );
        let other = database("v2.db", "CREATE TABLE session_message (id TEXT);");
        assert_eq!(
            opencode_usage(&other, 0, None),
            Ok(LocalUsageRead::NoHistory)
        );
        fs::remove_file(other).unwrap();
    }

    #[test]
    fn a_message_that_is_not_json_does_not_make_the_whole_meter_unreadable() {
        let valid = r#"{"role":"assistant","providerID":"zai","cost":0.5,"tokens":{"input":100,"output":10,"reasoning":5}}"#;
        let rows = [
            message("m1", "s1", 2_000, valid),
            // a truncated write and a plain word, both inside the window and the session
            message(
                "m2",
                "s1",
                2_000,
                r#"{"role":"assistant","tokens":{"input":9"#,
            ),
            message("m3", "s1", 2_000, "not json at all"),
            message("m4", "s1", 3_000, valid),
        ]
        .concat();
        let path = database(
            "malformed.db",
            &format!(
                "{OPENCODE_SCHEMA}
                 INSERT INTO session VALUES ('s1', 3000);
                 {rows}"
            ),
        );
        assert_eq!(
            opencode_usage(&path, 1_000, Some("zai")),
            Ok(LocalUsageRead::Totals(LocalUsage {
                tokens: 230,
                cost_usd: 1.0
            }))
        );
        fs::remove_file(path).unwrap();
    }

    /// A query that would run for minutes: every row of a million, each joined to a million.
    const ENDLESS_QUERY: &str = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000000) \
         SELECT count(*) FROM n a, n b";

    #[test]
    fn a_query_past_the_deadline_is_interrupted_by_sqlite() {
        let path = database("slow.db", "CREATE TABLE t (x);");
        let connection = open_live(&path, Instant::now() + Duration::from_millis(50)).unwrap();
        let started = Instant::now();
        let error = connection
            .query_row(ENDLESS_QUERY, [], |row| row.get::<_, i64>(0))
            .unwrap_err();
        assert_eq!(Unreadable::of(&error).0, "OperationInterrupted");
        assert!(started.elapsed() < Duration::from_secs(10));
        // a deadline still ahead leaves an ordinary query alone
        let connection = open_live(&path, Instant::now() + Duration::from_secs(60)).unwrap();
        assert_eq!(
            connection.query_row("SELECT 7", [], |row| row.get::<_, i64>(0)),
            Ok(7)
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_read_that_never_answers_gives_the_caller_back_after_the_limit() {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let (release, held) = mpsc::channel::<()>();
        let started = Instant::now();
        let outcome = bounded(Duration::from_millis(100), &COUNT, move |_| {
            // stands for a system call that does not return
            let _ = held.recv();
            Ok(())
        });
        assert_eq!(outcome, Err(Unreadable("timeout".to_owned())));
        assert!(started.elapsed() < Duration::from_secs(5));
        // the stuck read still counts until it ends
        assert_eq!(COUNT.load(Ordering::Acquire), 1);
        drop(release);
        let ended = Instant::now();
        while COUNT.load(Ordering::Acquire) != 0 {
            assert!(
                ended.elapsed() < Duration::from_secs(5),
                "the read never ended"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn reads_stuck_in_the_file_system_do_not_pile_up_threads() {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let (release, held) = mpsc::channel::<()>();
        let held = std::sync::Arc::new(std::sync::Mutex::new(held));
        for _ in 0..MAX_STUCK_READS {
            let held = held.clone();
            let outcome = bounded(Duration::from_millis(20), &COUNT, move |_| {
                // ignored: the sender dropping is the signal to end
                let _ = held.lock().map(|held| held.recv());
                Ok(())
            });
            assert_eq!(outcome, Err(Unreadable("timeout".to_owned())));
        }
        assert_eq!(
            bounded(Duration::from_secs(5), &COUNT, |_| Ok(())),
            Err(Unreadable("stalled".to_owned()))
        );
        drop(release);
    }

    #[test]
    fn opencode_credentials_come_from_the_active_row_of_the_table() {
        let none = database("no-table.db", "CREATE TABLE other (id TEXT);");
        assert_eq!(
            opencode_credential(&none, "opencode"),
            Ok(StoredCredential::NoTable)
        );
        let schema = "CREATE TABLE credential (id TEXT PRIMARY KEY, integration_id TEXT,
             label TEXT NOT NULL, value TEXT NOT NULL, active INTEGER, time_created INTEGER NOT NULL);";
        let empty = database("empty.db", schema);
        assert_eq!(
            opencode_credential(&empty, "opencode"),
            Ok(StoredCredential::NoTable)
        );
        let path = database(
            "credential.db",
            &format!(
                "{schema}
                 INSERT INTO credential VALUES
                   ('a', 'opencode', 'old', '{{\"key\":\"old\"}}', 0, 5),
                   ('b', 'opencode', 'chosen', '{{\"key\":\"chosen\"}}', 1, 1),
                   ('c', 'opencode', 'newer', '{{\"key\":\"newer\"}}', 0, 9),
                   ('d', 'openai', 'other', '{{\"key\":\"other\"}}', 1, 2);"
            ),
        );
        assert_eq!(
            opencode_credential(&path, "opencode"),
            Ok(StoredCredential::Found(Secret::new(r#"{"key":"chosen"}"#)))
        );
        assert_eq!(
            opencode_credential(&path, "zai-coding-plan"),
            Ok(StoredCredential::NotInTable)
        );
        let missing =
            std::env::temp_dir().join(format!("sn-foreign-{}-gone", uuid::Uuid::new_v4()));
        assert_eq!(
            opencode_credential(&missing, "opencode"),
            Ok(StoredCredential::NoTable)
        );
        for file in [none, empty, path] {
            fs::remove_file(file).unwrap();
        }
    }

    #[test]
    fn cookie_stores_return_only_the_domain_and_its_subdomains() {
        let chromium = database(
            "Cookies",
            "CREATE TABLE meta (key TEXT, value TEXT); INSERT INTO meta VALUES ('version', '24');
             CREATE TABLE cookies (host_key TEXT, name TEXT, encrypted_value BLOB);
             INSERT INTO cookies VALUES ('.claude.ai', 'sessionKey', x'763130'),
               ('api.claude.ai', 'other', x'00'), ('example.com', 'nope', x'00');",
        );
        let found = chromium_cookies(&chromium, "claude.ai");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].encrypted, b"v10");
        assert_eq!(found[0].version, 24);
        let firefox = database(
            "cookies.sqlite",
            "CREATE TABLE moz_cookies (host TEXT, name TEXT, value TEXT);
             INSERT INTO moz_cookies VALUES ('cursor.com', 'WorkosCursorSessionToken', 's'),
               ('example.com', 'nope', 'x');",
        );
        let found = firefox_cookies(&firefox, "cursor.com");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value.expose(), "s");
        fs::remove_file(chromium).unwrap();
        fs::remove_file(firefox).unwrap();
    }
}
