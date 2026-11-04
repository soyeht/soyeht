//! The adapter package each built-in harness launches, as the registry snapshot pins it.

use super::registry::{builtin_entries, pinned_package};

/// The adapter package a built-in harness launches, and the version the snapshot pins.
#[derive(Debug)]
pub(crate) struct PinnedAdapter {
    pub id: String,
    pub package: String,
    #[cfg(test)]
    pub version: String,
}

/// Every built-in entry's pinned package: the one place a package name is read from.
pub(crate) fn pinned_adapter(id: &str) -> Option<&'static PinnedAdapter> {
    static PINNED: std::sync::OnceLock<Vec<PinnedAdapter>> = std::sync::OnceLock::new();
    PINNED
        .get_or_init(|| {
            // An invalid snapshot fails every catalogue read with its own error.
            builtin_entries()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|entry| {
                    let (package, _version) = entry
                        .launch
                        .args
                        .iter()
                        .find(|argument| argument.starts_with('@') && pinned_package(argument))
                        .and_then(|argument| argument.rsplit_once('@'))?;
                    Some(PinnedAdapter {
                        id: entry.launch.id.clone(),
                        package: package.into(),
                        #[cfg(test)]
                        version: _version.into(),
                    })
                })
                .collect()
        })
        .iter()
        .find(|adapter| adapter.id == id)
}
