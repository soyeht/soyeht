//! Which profile a program runs as, and where that profile keeps its data.
//!
//! This is the one place that picks a [`Profile`] and joins its generated data subpath with the
//! base directory of the system. The caller reads the environment and the base directory
//! (`soyeht_platform::dirs::data_base()`), so the rules stay free of the environment and
//! testable.

use crate::{
    Error, Result,
    profile::{BUILD_PROFILE, PROFILES, Profile},
};
use std::{ffi::OsStr, path::PathBuf};

/// The option with which a program that starts the engine gives it the supervisor's program, so
/// the engine starts the supervisor again when it loses it (ADR 0108). A program that keeps the
/// supervisor alive another way, such as a launch agent, does not pass it.
pub const RESTART_SUPERVISOR_FLAG: &str = "--restart-supervisor";

/// The profile a program runs as: the one `requested` names (the value of
/// [`SELECTION_ENV`](crate::profile::SELECTION_ENV)), else the one the build defaults to
/// (`SOYEHT_BUILD_PROFILE` when it was compiled), else Dev. An empty value names nothing.
pub fn selected_profile(requested: Option<&OsStr>) -> Result<&'static Profile> {
    selection(requested, BUILD_PROFILE)
}

fn selection(requested: Option<&OsStr>, build: Option<&str>) -> Result<&'static Profile> {
    let requested = requested.filter(|name| !name.is_empty());
    let name = match requested {
        Some(name) => name
            .to_str()
            .ok_or_else(|| Error::UnknownProfile(name.to_string_lossy().into_owned()))?,
        None => build
            .filter(|name| !name.is_empty())
            .unwrap_or(crate::profile::DEV.name),
    };
    PROFILES
        .into_iter()
        .find(|profile| profile.name == name)
        .ok_or_else(|| Error::UnknownProfile(name.into()))
}

/// The data directory of `profile` below `base`, the system's directory for application data.
pub fn data_directory(profile: &Profile, base: Option<PathBuf>) -> Result<PathBuf> {
    let mut directory = base.ok_or(Error::HomeMissing)?;
    directory.extend(profile.data_directory.native().split('/'));
    Ok(directory)
}

/// The profile a program runs as and its data directory: `directory` when the command line gave
/// one, else the profile's below `base`. A command line, a service definition and a test all
/// choose this way, so no program repeats the rule.
pub fn resolve(
    requested: Option<&OsStr>,
    directory: Option<PathBuf>,
    base: Option<PathBuf>,
) -> Result<(&'static Profile, PathBuf)> {
    let profile = selected_profile(requested)?;
    let directory = match directory {
        Some(directory) => directory,
        None => data_directory(profile, base)?,
    };
    Ok((profile, directory))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{DEV, PROD};

    fn name(profile: Result<&'static Profile>) -> &'static str {
        profile.unwrap().name
    }

    #[test]
    fn dev_is_the_default_and_the_environment_beats_the_build() {
        assert_eq!(name(selection(None, None)), "dev");
        assert_eq!(name(selection(Some("".as_ref()), None)), "dev");
        assert_eq!(name(selection(None, Some("prod"))), "prod");
        assert_eq!(name(selection(Some("dev".as_ref()), Some("prod"))), "dev");
        assert_eq!(name(selection(Some("prod".as_ref()), Some("dev"))), "prod");
    }

    #[test]
    fn an_unknown_name_is_an_error_not_a_silent_dev() {
        assert!(matches!(
            selection(Some("production".as_ref()), None),
            Err(Error::UnknownProfile(name)) if name == "production"
        ));
        assert!(matches!(
            selection(None, Some("Prod")),
            Err(Error::UnknownProfile(name)) if name == "Prod"
        ));
    }

    fn known_names() -> String {
        PROFILES
            .iter()
            .map(|profile| profile.name)
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn the_message_names_the_bad_value_and_every_known_profile() {
        let error = selection(Some("bogus".as_ref()), None).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                r#"the profile "bogus" is not known (known: {})"#,
                known_names()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_text_is_named_without_failing_the_message() {
        use std::os::unix::ffi::OsStrExt;
        let error = selection(Some(OsStr::from_bytes(b"pr\xffod")), None).unwrap_err();
        assert!(error.to_string().starts_with("the profile \"pr"));
        assert!(
            error
                .to_string()
                .contains(&format!("(known: {})", known_names()))
        );
    }

    #[test]
    fn the_profile_directory_lives_below_the_base_directory() {
        let base = PathBuf::from("/data");
        let dev = data_directory(&DEV, Some(base.clone())).unwrap();
        let prod = data_directory(&PROD, Some(base.clone())).unwrap();
        let native = |profile: &Profile| base.join(profile.data_directory.native());
        assert_eq!(dev, native(&DEV));
        assert_eq!(prod, native(&PROD));
        assert_ne!(dev, prod);
        assert!(
            dev.starts_with(&prod),
            "Dev keeps its data inside the production folder"
        );
    }

    #[test]
    fn an_explicit_directory_replaces_the_profiles_but_not_the_profile() {
        let base = Some(PathBuf::from("/data"));
        let (profile, directory) = resolve(
            Some("prod".as_ref()),
            Some(PathBuf::from("/elsewhere")),
            base.clone(),
        )
        .unwrap();
        assert_eq!(
            (profile.name, directory),
            ("prod", PathBuf::from("/elsewhere"))
        );
        let (profile, directory) = resolve(Some("dev".as_ref()), None, base).unwrap();
        assert_eq!(
            directory,
            PathBuf::from("/data").join(DEV.data_directory.native())
        );
        assert_eq!(profile.name, "dev");
        assert!(resolve(Some("nope".as_ref()), Some(PathBuf::from("/d")), None).is_err());
    }

    #[test]
    fn without_a_base_directory_there_is_no_default() {
        assert!(matches!(
            data_directory(&DEV, None),
            Err(Error::HomeMissing)
        ));
    }
}
