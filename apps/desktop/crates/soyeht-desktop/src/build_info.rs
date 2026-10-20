//! Which build this is: the package version and the short commit it was built from.

/// The package version, the one number the Mac app and the engine share (`0.1.0`).
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The short commit `build.rs` found, `unknown` when the build has no git history, or the value of
/// `SOYEHT_BUILD_COMMIT` when a packaging step sets it.
const COMMIT: &str = env!("SOYEHT_BUILD_COMMIT");

/// The build id Preferences shows: the version, then the commit as semver build metadata.
pub(crate) fn id() -> String {
    format!("{VERSION}+{COMMIT}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_the_version_then_the_commit() {
        let id = id();
        let (version, commit) = id.split_once('+').unwrap_or_default();
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        assert!(!commit.is_empty());
        assert!(
            commit == "unknown" || commit.chars().all(|digit| digit.is_ascii_hexdigit()),
            "{commit}"
        );
    }
}
