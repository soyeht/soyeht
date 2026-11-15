//! What a person was told about devices added to this computer, and the panes a device made
//! (ADR 0164).
//!
//! An engine that asks for the person's code records, with each device it pairs, when it was
//! added. The notice of a device added is durable: it stays until the viewer dismisses it, and a
//! viewer is a paired device or the person at the computer. The panes a device made are what the
//! revoke dialog offers to close, since revoking a device does not stop what its terminals started.
use crate::database::{Change, Result, Store};
use rusqlite::{OptionalExtension, params};
use serde::de::DeserializeOwned;
use soyeht_protocol::{Device, Pane};

/// The time, in seconds, up to which `viewer` dismissed the notice. A viewer that never did has none.
pub fn seen(store: &Store, viewer: &str) -> Result<Option<i64>> {
    Ok(store
        .connection()
        .query_row(
            "SELECT seen_unix FROM device_notice_marks WHERE viewer = ?1",
            [viewer],
            |row| row.get(0),
        )
        .optional()?)
}

/// Dismisses the notice of `viewer` through `through`. A mark only moves forward: dismissing an
/// older time than the one kept leaves it.
pub fn dismiss_in(change: &Change<'_>, viewer: &str, through: i64) -> Result<()> {
    change.connection().execute(
        "INSERT INTO device_notice_marks(viewer,seen_unix) VALUES (?1,?2) ON CONFLICT(viewer) DO UPDATE SET seen_unix = MAX(seen_unix, excluded.seen_unix)",
        params![viewer, through],
    )?;
    Ok(())
}

/// The paired devices that were given a time when they were added, which is what the notice is
/// made of. A device paired by an engine that does not date devices is not one of them.
pub fn dated_devices(store: &Store) -> Result<Vec<Device>> {
    payloads(
        store,
        "SELECT payload FROM devices WHERE revoked = 0 AND json_extract(payload,'$.addedAt') IS NOT NULL ORDER BY id",
        [],
    )
}

/// The open panes the connection of `device` made, in the order they were made.
pub fn panes_created_by(store: &Store, device: &str) -> Result<Vec<Pane>> {
    payloads(
        store,
        "SELECT payload FROM panes WHERE created_by = ?1 AND closed = 0 ORDER BY created_seq",
        [device],
    )
}

/// What `sql`, which selects one payload column, finds, as the values the engine wrote.
fn payloads<T: DeserializeOwned>(
    store: &Store,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<T>> {
    let mut statement = store.connection().prepare(sql)?;
    let texts = statement
        .query_map(parameters, |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    texts
        .iter()
        .map(|text| serde_json::from_str(text).map_err(Into::into))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A store in a folder of its own, and the folder.
    fn store() -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("sn-notices-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&path).unwrap();
        (store, path)
    }

    /// Runs `write` against the open connection, in the change a call would make.
    fn seed(
        store: &mut Store,
        key: &str,
        write: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<()>,
    ) {
        store
            .mutate("test", key, "x", |change| {
                write(change.connection())?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
    }

    fn dismiss(store: &mut Store, viewer: &str, through: i64) {
        store
            .mutate("test", &uuid::Uuid::new_v4().to_string(), "x", |change| {
                dismiss_in(change, viewer, through)?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
    }

    fn pane_json(id: &str) -> serde_json::Value {
        json!({"paneId": id, "workspaceId": "w", "name": id, "cwd": "/", "mode": "terminal",
               "state": "idle", "source": "none", "confidence": 1.0, "phrase": "",
               "generation": 0, "cols": 80, "rows": 24, "record": false})
    }

    fn pane_ids(store: &Store, device: &str) -> Vec<String> {
        let panes = panes_created_by(store, device).unwrap();
        panes.into_iter().map(|pane| pane.pane_id).collect()
    }

    #[test]
    fn a_mark_is_kept_per_viewer_and_only_moves_forward() {
        let (mut store, path) = store();
        assert_eq!(seen(&store, "local").unwrap(), None);
        dismiss(&mut store, "local", 100);
        dismiss(&mut store, "local", 40);
        dismiss(&mut store, "phone", 7);
        assert_eq!(seen(&store, "local").unwrap(), Some(100));
        assert_eq!(seen(&store, "phone").unwrap(), Some(7));
        dismiss(&mut store, "local", 250);
        assert_eq!(seen(&store, "local").unwrap(), Some(250));
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn only_the_unrevoked_devices_that_were_dated_make_the_notice() {
        let (mut store, path) = store();
        seed(&mut store, "devices", |connection| {
            for (id, payload, revoked) in [
                (
                    "a",
                    json!({"deviceId":"a","name":"A","fingerprint":"f","revoked":false,"addedAt":"2026-11-15T10:00:00Z","node":"box"}),
                    0,
                ),
                (
                    "b",
                    json!({"deviceId":"b","name":"B","fingerprint":"f","revoked":false}),
                    0,
                ),
                (
                    "c",
                    json!({"deviceId":"c","name":"C","fingerprint":"f","revoked":true,"addedAt":"2026-11-15T10:00:00Z"}),
                    1,
                ),
            ] {
                connection.execute(
                    "INSERT INTO devices(id,payload,public_key,token_hash,revoked) VALUES (?1,?2,x'01',?1,?3)",
                    params![id, payload.to_string(), revoked],
                )?;
            }
            Ok(())
        });
        let dated = dated_devices(&store).unwrap();
        assert_eq!(dated.len(), 1);
        assert_eq!(dated[0].device_id, "a");
        assert_eq!(dated[0].node.as_deref(), Some("box"));
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn a_pane_is_the_devices_only_when_its_connection_made_it_and_it_is_open() {
        let (mut store, path) = store();
        seed(&mut store, "panes", |connection| {
            connection.execute("INSERT INTO workspaces(id,payload) VALUES ('w','{}')", [])?;
            for (id, creator, closed) in [
                ("a", Some("phone"), 0),
                ("b", None, 0),
                ("c", Some("phone"), 1),
                ("d", Some("other"), 0),
                ("e", Some("phone"), 0),
            ] {
                connection.execute(
                    "INSERT INTO panes(id,workspace_id,payload,instance_key,closed,created_seq,created_by) VALUES (?1,'w',?2,?1,?3,?4,?5)",
                    params![id, pane_json(id).to_string(), closed, id.as_bytes()[0] as i64, creator],
                )?;
            }
            Ok(())
        });
        assert_eq!(pane_ids(&store, "phone"), ["a", "e"]);
        assert!(panes_created_by(&store, "nobody").unwrap().is_empty());
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn a_pane_a_device_makes_is_recorded_as_its_own_and_one_the_person_makes_is_nobody_s() {
        use crate::domain::PaneRecord;
        let (mut store, path) = store();
        seed(&mut store, "workspace", |connection| {
            connection
                .execute("INSERT INTO workspaces(id,payload) VALUES ('w','{}')", [])
                .map(drop)
        });
        let record = |id: &str| PaneRecord {
            pane: serde_json::from_value(pane_json(id)).unwrap(),
            closed: false,
            instance_key: id.into(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        };
        let phone = json!({"role": "human_device", "client": "phone", "deviceId": "phone"});
        let person = json!({"role": "human_local", "client": "app"});
        for (key, actor, id) in [("1", &phone, "by-phone"), ("2", &person, "by-person")] {
            store
                .mutate_as("test", key, "x", Some(actor), |change| {
                    change.insert_pane(&record(id))?;
                    Ok((json!({}), vec![]))
                })
                .unwrap();
        }
        assert_eq!(pane_ids(&store, "phone"), ["by-phone"]);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn a_database_of_version_36_gains_the_creator_column_and_the_marks_and_keeps_its_panes() {
        let (store, path) = store();
        drop(store);
        let connection = rusqlite::Connection::open(path.join("soyeht.db")).unwrap();
        connection
            .execute_batch(
                "DROP INDEX panes_by_creator; ALTER TABLE panes DROP COLUMN created_by;
                 DROP TABLE device_notice_marks; PRAGMA user_version = 36;
                 INSERT INTO workspaces(id,payload) VALUES ('w','{}');
                 INSERT INTO panes(id,workspace_id,payload,instance_key) VALUES ('old','w','{}','k');",
            )
            .unwrap();
        drop(connection);

        let mut store = Store::open(&path).unwrap();
        let creator: Option<String> = store
            .connection()
            .query_row("SELECT created_by FROM panes WHERE id = 'old'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(creator, None);
        dismiss(&mut store, "local", 5);
        assert_eq!(seen(&store, "local").unwrap(), Some(5));
        drop(store);
        // Opening it again changes nothing and does not fail on the column that is there.
        drop(Store::open(&path).unwrap());
        std::fs::remove_dir_all(path).unwrap();
    }
}
