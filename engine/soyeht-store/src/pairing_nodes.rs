//! The tailnet nodes devices connected from, and the tailnet identity a pairing pinned (ADR 0164).
//!
//! An engine that asks for the person's code records, when a paired device's authenticated
//! `hello` arrives, the Tailscale node it came from, and refuses to start a pairing from a node
//! that belongs to a device. It also pins the tailnet and this computer's node key the first time
//! it serves a pairing. No other engine writes any of it.
use crate::database::{Change, Result, Store};
use rusqlite::{OptionalExtension, params};

/// A device that was seen on a node.
#[derive(Debug, PartialEq, Eq)]
pub struct NodeUse {
    /// The device has been revoked.
    pub revoked: bool,
    /// When it was revoked, for a revoked device whose revocation recorded the time.
    pub revoked_unix: Option<i64>,
}

/// The tailnet and this computer's node key, as they were when the first pairing was served.
#[derive(Debug, PartialEq, Eq)]
pub struct PinnedTailnet {
    pub tailnet: String,
    pub node_key: String,
}

/// Every device seen on `node`.
pub fn uses_of(store: &Store, node: &str) -> Result<Vec<NodeUse>> {
    let mut statement = store.connection().prepare(
        "SELECT devices.revoked, device_nodes.revoked_unix FROM device_nodes JOIN devices ON devices.id = device_nodes.device_id WHERE device_nodes.node_id = ?1",
    )?;
    let rows = statement.query_map([node], |row| {
        Ok(NodeUse {
            revoked: row.get::<_, i64>(0)? != 0,
            revoked_unix: row.get(1)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Records that `device` connected from `node` at `now`. A device that is revoked or unknown is
/// not recorded: its revocation has already stamped the nodes it had.
pub fn record(store: &Store, device: &str, node: &str, now: i64) -> Result<()> {
    record_on(store.connection(), device, node, now)
}

/// [`record`] inside the change that makes the device, for the node its pairing came from.
pub fn record_in(change: &Change<'_>, device: &str, node: &str, now: i64) -> Result<()> {
    record_on(change.connection(), device, node, now)
}

fn record_on(connection: &rusqlite::Connection, device: &str, node: &str, now: i64) -> Result<()> {
    connection.execute(
        "INSERT INTO device_nodes(device_id,node_id,first_seen_unix,last_seen_unix) SELECT id, ?2, ?3, ?3 FROM devices WHERE id = ?1 AND revoked = 0 ON CONFLICT(device_id,node_id) DO UPDATE SET last_seen_unix = excluded.last_seen_unix",
        params![device, node, now],
    )?;
    Ok(())
}

/// Stamps the nodes of `device` with the time of its revocation, inside the revocation's change.
pub fn mark_revoked(change: &Change<'_>, device: &str, now: i64) -> Result<()> {
    change.connection().execute(
        "UPDATE device_nodes SET revoked_unix = ?2 WHERE device_id = ?1",
        params![device, now],
    )?;
    Ok(())
}

/// The tailnet identity pinned by the first pairing served, when there is one.
pub fn pinned(store: &Store) -> Result<Option<PinnedTailnet>> {
    Ok(store
        .connection()
        .query_row(
            "SELECT tailnet, node_key FROM pairing_tailnet WHERE singleton = 1",
            [],
            |row| {
                Ok(PinnedTailnet {
                    tailnet: row.get(0)?,
                    node_key: row.get(1)?,
                })
            },
        )
        .optional()?)
}

/// Pins `tailnet` and `node_key`, in place of what was pinned.
pub fn pin(store: &Store, tailnet: &str, node_key: &str, now: i64) -> Result<()> {
    store.connection().execute(
        "INSERT INTO pairing_tailnet(singleton,tailnet,node_key,pinned_unix) VALUES (1,?1,?2,?3) ON CONFLICT(singleton) DO UPDATE SET tailnet = excluded.tailnet, node_key = excluded.node_key, pinned_unix = excluded.pinned_unix",
        params![tailnet, node_key, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("sn-pairing-nodes-{}", uuid::Uuid::new_v4()));
        (Store::open(&path).unwrap(), path)
    }

    fn add_device(store: &mut Store, id: &str) {
        store
            .mutate("test", id, id, |change| {
                change.connection().execute(
                    "INSERT INTO devices(id,payload,public_key,token_hash) VALUES (?1,'{}',x'01',?1)",
                    [id],
                )?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
    }

    #[test]
    fn a_node_is_remembered_per_device_and_a_revoked_or_unknown_device_is_not_recorded() {
        let (mut store, path) = store();
        add_device(&mut store, "phone");
        assert!(uses_of(&store, "n1").unwrap().is_empty());
        record(&store, "phone", "n1", 10).unwrap();
        record(&store, "phone", "n1", 20).unwrap();
        record(&store, "ghost", "n2", 20).unwrap();
        assert_eq!(
            uses_of(&store, "n1").unwrap(),
            [NodeUse {
                revoked: false,
                revoked_unix: None
            }]
        );
        assert!(uses_of(&store, "n2").unwrap().is_empty());
        let seen: (i64, i64) = store
            .connection()
            .query_row(
                "SELECT first_seen_unix,last_seen_unix FROM device_nodes",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(seen, (10, 20));
        store
            .mutate("test", "revoke", "revoke", |change| {
                change
                    .connection()
                    .execute("UPDATE devices SET revoked = 1 WHERE id = 'phone'", [])?;
                mark_revoked(change, "phone", 30)?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
        assert_eq!(
            uses_of(&store, "n1").unwrap(),
            [NodeUse {
                revoked: true,
                revoked_unix: Some(30)
            }]
        );
        // A revoked device is not recorded again.
        record(&store, "phone", "n3", 40).unwrap();
        assert!(uses_of(&store, "n3").unwrap().is_empty());
        drop(store);
        // ignored: the scratch folder of a test; the system clears the temporary folder.
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn a_node_can_be_recorded_in_the_change_that_makes_the_device() {
        let (mut store, path) = store();
        store
            .mutate("test", "pair", "pair", |change| {
                change.connection().execute(
                    "INSERT INTO devices(id,payload,public_key,token_hash) VALUES ('new','{}',x'01','new')",
                    [],
                )?;
                record_in(change, "new", "n-new", 5)?;
                // A change that records a node for a device it did not make records nothing.
                record_in(change, "other", "n-other", 5)?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
        assert_eq!(uses_of(&store, "n-new").unwrap().len(), 1);
        assert!(uses_of(&store, "n-other").unwrap().is_empty());
        drop(store);
        // ignored: the scratch folder of a test; the system clears the temporary folder.
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn the_pin_is_one_row_that_a_new_pin_replaces() {
        let (store, path) = store();
        assert_eq!(pinned(&store).unwrap(), None);
        pin(&store, "a.example", "nodekey:1", 1).unwrap();
        pin(&store, "b.example", "nodekey:2", 2).unwrap();
        assert_eq!(
            pinned(&store).unwrap(),
            Some(PinnedTailnet {
                tailnet: "b.example".into(),
                node_key: "nodekey:2".into()
            })
        );
        assert!(pin(&store, "", "k", 3).is_err());
        drop(store);
        // ignored: the scratch folder of a test; the system clears the temporary folder.
        let _ = std::fs::remove_dir_all(path);
    }
}
