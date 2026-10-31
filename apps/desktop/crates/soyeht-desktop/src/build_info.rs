//! Which build this is: the package version and the short commit it was built from.

use soyeht_protocol::profile::{DEV, Profile};

/// The package version, the one number the Mac app and the engine share (`0.1.0`).
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The short commit `build.rs` found, `unknown` when the build has no git history, or the value of
/// `SOYEHT_BUILD_COMMIT` when a packaging step sets it.
const COMMIT: &str = env!("SOYEHT_BUILD_COMMIT");

/// The build id Preferences shows: the version, then the commit as semver build metadata.
pub(crate) fn id() -> String {
    format!("{VERSION}+{COMMIT}")
}

/// What Preferences says of the profile. A Dev build names it, so a person who runs it beside the
/// installed app does not take one for the other; the production build is the product and says
/// nothing of it.
pub(crate) fn profile_note() -> Option<&'static str> {
    note_for(soyeht_client::profile::selected().ok()?)
}

fn note_for(profile: &Profile) -> Option<&'static str> {
    (profile.name == DEV.name).then_some("Dev profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_dev_build_names_its_profile() {
        assert_eq!(note_for(&DEV), Some("Dev profile"));
        assert_eq!(note_for(&soyeht_protocol::profile::PROD), None);
    }

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
