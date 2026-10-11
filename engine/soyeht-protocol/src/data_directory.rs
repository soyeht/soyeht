//! Where the Dev profile keeps its data, derived from the user's home directory.
//!
//! This is the one place that joins the home directory with the generated
//! `profile::DEV_DATA_DIRECTORY_FROM_HOME`. The caller reads the home directory
//! (`std::env::home_dir()`), so the rule stays free of the environment and testable.

use crate::{Error, Result, profile::DEV_DATA_DIRECTORY_FROM_HOME};
use std::path::PathBuf;

/// The default data directory of the Dev profile for a user whose home is `home`.
pub fn dev_data_directory(home: Option<PathBuf>) -> Result<PathBuf> {
    home.map(|home| home.join(DEV_DATA_DIRECTORY_FROM_HOME))
        .ok_or(Error::HomeMissing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_directory_lives_under_the_home_directory() {
        assert_eq!(
            dev_data_directory(Some("/home/a".into())).unwrap(),
            PathBuf::from("/home/a").join(DEV_DATA_DIRECTORY_FROM_HOME)
        );
    }

    #[test]
    fn without_a_home_directory_there_is_no_default() {
        assert!(matches!(dev_data_directory(None), Err(Error::HomeMissing)));
    }
}
