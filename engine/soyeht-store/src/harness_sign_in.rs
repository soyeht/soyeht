//! The protocol sign-in method the person chose for a harness (ADR 0100).
//!
//! One row per harness holds the method id the person pressed, so a new pane of the harness,
//! and a switch into it, authenticate their agent process without another press. Only the
//! method id is stored: never a credential.
use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, Transaction, params};

const ID_CHARS: usize = 128;
const METHOD_CHARS: usize = 256;

fn valid(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.chars().count() <= limit
}

/// The method the person chose for `harness`, if any.
pub fn chosen(store: &Store, harness: &str) -> Result<Option<String>> {
    Ok(store
        .connection()
        .query_row(
            "SELECT method_id FROM harness_sign_in WHERE harness_id=?1",
            [harness],
            |row| row.get(0),
        )
        .optional()?)
}

/// Keeps `method` as the harness's sign-in, replacing an earlier choice.
pub fn remember(change: &Change<'_>, harness: &str, method: &str) -> Result<()> {
    if !valid(harness, ID_CHARS) || !valid(method, METHOD_CHARS) {
        return Err(Error::InvalidHarnessSignIn);
    }
    change.connection().execute(
        "INSERT INTO harness_sign_in(harness_id,method_id) VALUES (?1,?2) ON CONFLICT(harness_id) DO UPDATE SET method_id=excluded.method_id",
        params![harness, method],
    )?;
    Ok(())
}

/// Forgets the harness's sign-in: the agent refused it, or the person signed out.
pub fn forget(change: &Change<'_>, harness: &str) -> Result<()> {
    change
        .connection()
        .execute("DELETE FROM harness_sign_in WHERE harness_id=?1", [harness])?;
    Ok(())
}

/// Before version 31 the choice lived in the runtime state of each pane. A choice of a pane
/// that is still open becomes its harness's, and the pane's copy is removed.
pub(crate) fn adopt_pane_choices(tx: &Transaction<'_>) -> Result<()> {
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='agent_runtime')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    tx.execute(
        "INSERT OR IGNORE INTO harness_sign_in(harness_id,method_id) \
         SELECT json_extract(r.payload,'$.launch.id'), json_extract(r.payload,'$.agent_sign_in') \
         FROM agent_runtime r JOIN panes p ON p.id=r.pane_id \
         WHERE p.closed=0 \
           AND json_type(r.payload,'$.launch.id')='text' \
           AND json_type(r.payload,'$.agent_sign_in')='text' \
           AND length(json_extract(r.payload,'$.launch.id')) BETWEEN 1 AND ?1 \
           AND length(json_extract(r.payload,'$.agent_sign_in')) BETWEEN 1 AND ?2 \
         ORDER BY r.pane_id",
        params![ID_CHARS as i64, METHOD_CHARS as i64],
    )?;
    tx.execute(
        "UPDATE agent_runtime SET payload=json_remove(payload,'$.agent_sign_in') \
         WHERE json_type(payload,'$.agent_sign_in') IS NOT NULL",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;

    fn on_store(run: impl FnOnce(&std::path::Path)) {
        let path = std::env::temp_dir().join(format!("sn-sign-in-{}", uuid::Uuid::new_v4()));
        run(&path);
        let _ = std::fs::remove_dir_all(path);
    }

    fn apply(store: &mut Store, key: &str, write: impl FnOnce(&Change<'_>) -> Result<()>) {
        store
            .mutate("test", key, key, |change| {
                write(change)?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
    }

    #[test]
    fn a_choice_is_replaced_forgotten_and_survives_reopen() {
        on_store(|path| {
            let mut store = Store::open(path).unwrap();
            assert_eq!(chosen(&store, "per-process-auth").unwrap(), None);
            apply(&mut store, "first", |change| {
                remember(change, "per-process-auth", "existing-login")?;
                remember(change, "other", "browser")?;
                remember(change, "per-process-auth", "api-key")
            });
            drop(store);
            let mut store = Store::open(path).unwrap();
            assert_eq!(
                chosen(&store, "per-process-auth").unwrap().as_deref(),
                Some("api-key")
            );
            apply(&mut store, "forget", |change| {
                forget(change, "per-process-auth")
            });
            assert_eq!(chosen(&store, "per-process-auth").unwrap(), None);
            assert_eq!(chosen(&store, "other").unwrap().as_deref(), Some("browser"));
        });
    }

    #[test]
    fn an_empty_or_oversized_value_is_refused() {
        on_store(|path| {
            let mut store = Store::open(path).unwrap();
            apply(&mut store, "bad", |change| {
                for (harness, method) in [("", "m"), ("h", ""), ("h", &"m".repeat(257))] {
                    assert!(matches!(
                        remember(change, harness, method),
                        Err(Error::InvalidHarnessSignIn)
                    ));
                }
                Ok(())
            });
            assert_eq!(chosen(&store, "h").unwrap(), None);
        });
    }

    #[test]
    fn migration_from_v30_moves_the_choice_of_each_open_pane_to_its_harness() {
        on_store(|path| {
            drop(Store::open(path).unwrap());
            let connection = Connection::open(path.join("soyeht.db")).unwrap();
            connection
                .execute_batch(
                    "DROP TABLE harness_sign_in; PRAGMA user_version=30;
                     INSERT INTO workspaces(id,payload) VALUES ('w','{}');
                     INSERT INTO panes(id,workspace_id,payload,instance_key,closed)
                       VALUES ('open','w','{}','k1',0),('shut','w','{}','k2',1);
                     INSERT INTO agent_runtime(pane_id,generation,payload) VALUES
                       ('open',1,'{\"launch\":{\"id\":\"per-process-auth\"},\"agent_sign_in\":\"existing-login\"}'),
                       ('shut',1,'{\"launch\":{\"id\":\"closed-harness\"},\"agent_sign_in\":\"stale\"}');",
                )
                .unwrap();
            drop(connection);

            let store = Store::open(path).unwrap();
            assert_eq!(
                chosen(&store, "per-process-auth").unwrap().as_deref(),
                Some("existing-login")
            );
            assert_eq!(chosen(&store, "closed-harness").unwrap(), None);
            let kept: i64 = store
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM agent_runtime WHERE json_type(payload,'$.agent_sign_in') IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(kept, 0);
        });
    }
}
