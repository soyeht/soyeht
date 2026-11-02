//! Read-only access to other applications' SQLite databases: the Cursor app's state database,
//! the browsers' cookie stores (CodexBar `CursorAppAuth`, its cookie import) and OpenCode's
//! database (its credential table and the usage its sessions recorded).
//!
//! This is the only module outside `soyeht-store` that opens SQLite, a documented exception in
//! the hardening gate (ADR 0077). Every database is opened read-only, with `immutable=1` where
//! the owning application may hold it open, so no lock is taken and nothing is ever written.

use crate::host::{Cookie, LocalUsage, LocalUsageRead, Secret, SecretRead, StoredCredential};
use rusqlite::{Connection, OpenFlags, OptionalExtension, types::ValueRef};
use std::{path::Path, time::Duration};

/// How long a state-database read waits for the owning application's write lock.
const BUSY_WAIT: Duration = Duration::from_millis(250);

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
fn open_live(path: &Path) -> rusqlite::Result<Connection> {
    let connection =
        Connection::open_with_flags(path, flags()).or_else(|_| open_immutable(path))?;
    // ignored: a connection that cannot set its wait fails fast, and the read reports it
    let _ = connection.busy_timeout(BUSY_WAIT);
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
/// secret out of it.
pub fn opencode_credential(path: &Path, integration: &str) -> Result<StoredCredential, Unreadable> {
    if !path.is_file() {
        return Ok(StoredCredential::NoTable);
    }
    let connection = open_live(path).map_err(|error| Unreadable::of(&error))?;
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
/// through its session index and not scanned.
pub fn opencode_usage(
    path: &Path,
    since_ms: i64,
    provider: Option<&str>,
) -> Result<LocalUsageRead, Unreadable> {
    if !path.is_file() {
        return Ok(LocalUsageRead::NoDatabase);
    }
    let connection = open_live(path).map_err(|error| Unreadable::of(&error))?;
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
