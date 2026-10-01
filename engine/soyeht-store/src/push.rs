//! APNs registrations of paired devices.
//!
//! A registration belongs to one unrevoked device. Revocation deletes it in
//! the same transaction (see `Change::revoke_device`), so a revoked phone never
//! receives another notification.

use crate::database::{Change, Result, Store};
use rusqlite::params;
use soyeht_protocol::PushEnvironment;

#[derive(Clone, Debug, PartialEq)]
pub struct PushRegistration {
    pub device_id: String,
    pub token: String,
    pub environment: PushEnvironment,
}

fn environment_name(environment: &PushEnvironment) -> &'static str {
    match environment {
        PushEnvironment::Sandbox => "sandbox",
        PushEnvironment::Production => "production",
    }
}

fn environment_from_name(name: &str) -> rusqlite::Result<PushEnvironment> {
    match name {
        "sandbox" => Ok(PushEnvironment::Sandbox),
        "production" => Ok(PushEnvironment::Production),
        other => Err(rusqlite::Error::InvalidColumnType(
            2,
            other.into(),
            rusqlite::types::Type::Text,
        )),
    }
}

impl Store {
    /// Registrations of every unrevoked device, in device order.
    pub fn push_registrations(&self) -> Result<Vec<PushRegistration>> {
        let mut statement = self.connection.prepare(
            "SELECT p.device_id,p.token,p.environment FROM push_registrations p \
             JOIN devices d ON d.id=p.device_id WHERE d.revoked=0 ORDER BY p.device_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(PushRegistration {
                device_id: row.get(0)?,
                token: row.get(1)?,
                environment: environment_from_name(&row.get::<_, String>(2)?)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

impl Change<'_> {
    /// Stores the device's current token. A token moves to the newest device
    /// that registers it, which happens when a phone pairs again.
    pub fn register_push(
        &self,
        device_id: &str,
        token: &str,
        environment: &PushEnvironment,
        now_unix: i64,
    ) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM push_registrations WHERE token=?1 AND device_id<>?2",
            params![token, device_id],
        )?;
        self.transaction.execute(
            "INSERT INTO push_registrations(device_id,token,environment,updated_unix) VALUES (?1,?2,?3,?4) \
             ON CONFLICT(device_id) DO UPDATE SET token=excluded.token,environment=excluded.environment,updated_unix=excluded.updated_unix",
            params![device_id, token, environment_name(environment), now_unix],
        )?;
        Ok(())
    }

    pub fn unregister_push(&self, device_id: &str) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM push_registrations WHERE device_id=?1",
            [device_id],
        )?;
        Ok(())
    }

    /// Drops a token APNs reported as no longer valid, unless the device has
    /// registered a different one since.
    pub fn forget_push_token(&self, device_id: &str, token: &str) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM push_registrations WHERE device_id=?1 AND token=?2",
            params![device_id, token],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PushEnvironment::{Production, Sandbox};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    /// Runs `body` on a fresh store and removes its directory afterwards.
    fn on_store(body: impl FnOnce(&Path, Store)) {
        let path: PathBuf = std::env::temp_dir().join(format!("sn-push-{}", uuid::Uuid::new_v4()));
        body(&path, Store::open(&path).unwrap());
        std::fs::remove_dir_all(path).unwrap();
    }

    fn apply(store: &mut Store, step: impl FnOnce(&Change<'_>) -> Result<()>) {
        let key = uuid::Uuid::new_v4().to_string();
        store
            .mutate("test", &key, &key, |change| {
                step(change)?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
    }

    fn pair(store: &mut Store, device: &str) {
        apply(store, |change| {
            change.open_pair_window(device, i64::MAX, 0)?;
            change.consume_pair_window_and_insert_device(
                device,
                device,
                &json!({"deviceId":device,"name":device,"fingerprint":"f","revoked":false}),
                device.as_bytes(),
                device.as_bytes(),
            )
        });
    }

    fn register(store: &mut Store, device: &str, token: &str, environment: PushEnvironment) {
        apply(store, |change| {
            change.register_push(device, token, &environment, 1)
        });
    }

    /// A store with a phone registered for the sandbox and an unregistered tablet.
    fn on_paired_store(body: impl FnOnce(Store)) {
        on_store(|_, mut store| {
            pair(&mut store, "phone");
            pair(&mut store, "tablet");
            register(&mut store, "phone", "aa11", Sandbox);
            body(store);
        });
    }

    fn rows(store: &Store) -> Vec<(String, String, PushEnvironment)> {
        store
            .push_registrations()
            .unwrap()
            .into_iter()
            .map(|row| (row.device_id, row.token, row.environment))
            .collect()
    }

    fn row(
        device: &str,
        token: &str,
        environment: PushEnvironment,
    ) -> (String, String, PushEnvironment) {
        (device.into(), token.into(), environment)
    }

    #[test]
    fn one_registration_per_device_and_a_token_follows_the_newest_device() {
        on_paired_store(|mut store| {
            register(&mut store, "tablet", "bb22", Production);
            register(&mut store, "phone", "cc33", Production);
            assert_eq!(
                rows(&store),
                vec![
                    row("phone", "cc33", Production),
                    row("tablet", "bb22", Production)
                ]
            );
            register(&mut store, "tablet", "cc33", Sandbox);
            assert_eq!(rows(&store), vec![row("tablet", "cc33", Sandbox)]);
        });
    }

    #[test]
    fn stale_tokens_unregistering_and_revocation_remove_registrations() {
        on_paired_store(|mut store| {
            apply(&mut store, |c| c.forget_push_token("phone", "old"));
            assert_eq!(rows(&store).len(), 1, "a newer token survives");
            apply(&mut store, |c| c.forget_push_token("phone", "aa11"));
            register(&mut store, "tablet", "bb22", Sandbox);
            apply(&mut store, |c| c.unregister_push("tablet"));
            assert!(rows(&store).is_empty());
            register(&mut store, "tablet", "bb22", Sandbox);
            apply(&mut store, |c| {
                c.revoke_device("tablet", &json!({"revoked":true}))
            });
            assert!(rows(&store).is_empty());
        });
    }

    #[test]
    fn migration_from_v27_keeps_paired_devices() {
        on_store(|path, mut store| {
            pair(&mut store, "phone");
            store
                .connection()
                .execute_batch("DROP TABLE push_registrations; PRAGMA user_version = 27;")
                .unwrap();
            drop(store);
            let mut store = Store::open(path).unwrap();
            let version: u32 = store
                .connection()
                .pragma_query_value(None, "user_version", |value| value.get(0))
                .unwrap();
            assert_eq!(version, 28);
            assert!(
                store
                    .device::<serde_json::Value>("phone")
                    .unwrap()
                    .is_some()
            );
            register(&mut store, "phone", "aa11", Sandbox);
            assert_eq!(rows(&store), vec![row("phone", "aa11", Sandbox)]);
            assert!(
                std::fs::read_dir(path.join("backups"))
                    .unwrap()
                    .next()
                    .is_some()
            );
        });
    }
}
