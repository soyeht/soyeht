//! The packages a person acknowledged a built-in adapter downloads and runs unchecked (ADR 0143).
//!
//! One row per `name@version`, written by the request of a person who was told what the first
//! start downloads. A new version of the adapter is another package, so it asks again.
use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, params};

const PACKAGE_CHARS: usize = 256;

/// Whether `package` (`name@version`) was acknowledged.
pub fn acknowledged(store: &Store, package: &str) -> Result<bool> {
    Ok(store
        .connection()
        .query_row(
            "SELECT 1 FROM adapter_acknowledgements WHERE package=?1",
            [package],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Keeps `package` as acknowledged at `at_ms`. The first acknowledgement stands.
pub fn acknowledge(change: &Change<'_>, package: &str, at_ms: i64) -> Result<()> {
    if package.is_empty() || package.chars().count() > PACKAGE_CHARS {
        return Err(Error::InvalidAdapterAcknowledgement);
    }
    change.connection().execute(
        "INSERT OR IGNORE INTO adapter_acknowledgements(package,acknowledged_ms) VALUES (?1,?2)",
        params![package, at_ms],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Acknowledges every package of `packages` in one change.
    fn acknowledge_all(store: &mut Store, packages: &[&str]) -> Result<()> {
        store
            .mutate("test", "ack", "ack", |change| {
                for package in packages {
                    acknowledge(change, package, 10)?;
                }
                Ok((json!({}), vec![]))
            })
            .map(drop)
    }

    #[test]
    fn an_acknowledgement_is_of_one_exact_package_and_survives_reopen() {
        let path = std::env::temp_dir().join(format!("sn-adapter-ack-{}", uuid::Uuid::new_v4()));
        let mut store = Store::open(&path).unwrap();
        assert!(!acknowledged(&store, "@google/gemini-cli@0.59.0").unwrap());
        let twice = ["@google/gemini-cli@0.59.0", "@google/gemini-cli@0.59.0"];
        acknowledge_all(&mut store, &twice).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        assert!(acknowledged(&store, "@google/gemini-cli@0.59.0").unwrap());
        // Another version of the same adapter is another package.
        assert!(!acknowledged(&store, "@google/gemini-cli@0.60.0").unwrap());
        let count: i64 = store
            .connection()
            .query_row("SELECT COUNT(*) FROM adapter_acknowledgements", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        drop(store);
        // ignored: the scratch folder of a test; the system clears the temporary folder.
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn an_empty_or_oversized_package_is_refused() {
        let path = std::env::temp_dir().join(format!("sn-adapter-ack-{}", uuid::Uuid::new_v4()));
        let mut store = Store::open(&path).unwrap();
        for package in ["", &"p".repeat(257)] {
            let refused = acknowledge_all(&mut store, &[package]);
            assert!(matches!(refused, Err(Error::InvalidAdapterAcknowledgement)));
        }
        drop(store);
        // ignored: the scratch folder of a test; the system clears the temporary folder.
        let _ = std::fs::remove_dir_all(path);
    }
}
