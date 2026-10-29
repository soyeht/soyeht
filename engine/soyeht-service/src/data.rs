//! Deleting a profile's data: the one thing an uninstall keeps unless the person asks.
//!
//! The data directory holds the database (workspaces, transcripts), the sockets and the logs.
//! Deleting it is irreversible, so this module refuses whatever could be somebody else's: a
//! place that is not the app's own (no "next", or the legacy app's), the home folder or a folder
//! above it, a link, and a directory whose programs still answer. A profile's directory can hold
//! another profile's (Dev is inside prod's), and that one is kept.

use crate::{
    error::{Error, Result},
    guard,
    layout::Layout,
};
use soyeht_platform::local::connect;
use soyeht_protocol::{
    data_directory::data_directory,
    profile::{ENGINE_SOCKET, PROFILES, PTYD_SOCKET},
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Deletes the data directory of `layout`, except the data directory of any other profile that
/// lies inside it (found below `base`, the system's folder for application data). Returns
/// whether there was a directory to delete.
///
/// # Errors
/// [`Error::Legacy`] for a place that is not the app's own, [`Error::Unsafe`] for a link,
/// [`Error::StillRunning`] when the engine or the supervisor answers on its endpoint, and
/// [`Error::Io`] when a file cannot be removed.
pub fn remove_data(layout: &Layout, base: &Path) -> Result<bool> {
    let data = layout.data.as_path();
    guard::require_path("data directory", data)?;
    if layout.home.starts_with(data) {
        return Err(Error::Legacy(format!(
            "the data directory \"{}\" holds the home folder",
            data.display()
        )));
    }
    match fs::symlink_metadata(data) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(Error::Unsafe("the data directory")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    }
    for socket in [ENGINE_SOCKET, PTYD_SOCKET] {
        if connect(&data.join(socket)).is_ok() {
            return Err(Error::StillRunning);
        }
    }
    let mut keep = Vec::new();
    for profile in PROFILES.iter().filter(|p| p.name != layout.profile.name) {
        let other = data_directory(profile, Some(base.to_path_buf()))
            .map_err(|_| Error::Environment("the data folder"))?;
        if other.starts_with(data) && other != data {
            keep.push(other);
        }
    }
    remove_except(data, &keep)?;
    Ok(true)
}

/// Removes `path` with everything in it, except the paths of `keep`; a folder that holds a kept
/// one is emptied of the rest and stays.
fn remove_except(path: &Path, keep: &[PathBuf]) -> io::Result<()> {
    if keep.iter().all(|kept| !kept.starts_with(path)) {
        return remove_all(path);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?.path();
        if !keep.contains(&entry) {
            remove_except(&entry, keep)?;
        }
    }
    Ok(())
}

/// Removes a file, a link (not what it points to) or a folder with everything in it.
fn remove_all(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{guard::legacy_values, layout::test_layout, system::System, testing::Scratch};
    use soyeht_protocol::profile::{DEV, PROD};

    fn data_layout(scratch: &Scratch) -> Layout {
        let layout = test_layout(System::current(), &scratch.0);
        fs::create_dir_all(&layout.data).unwrap();
        layout
    }

    fn base(scratch: &Scratch) -> PathBuf {
        scratch.0.join("base")
    }

    fn fill(data: &Path) {
        fs::create_dir_all(data.join("logs")).unwrap();
        fs::write(data.join("logs/engine.log"), "line").unwrap();
        fs::write(data.join("soyeht.db"), "rows").unwrap();
    }

    #[test]
    fn the_data_directory_is_deleted_with_everything_in_it() {
        let scratch = Scratch::short("all");
        let layout = data_layout(&scratch);
        fill(&layout.data);
        assert!(remove_data(&layout, &base(&scratch)).unwrap());
        assert!(!layout.data.exists());
        // The folders around it are not the seam's to remove.
        assert!(layout.data.parent().unwrap().is_dir());
    }

    #[test]
    fn a_directory_that_is_not_there_is_nothing_to_delete() {
        let scratch = Scratch::short("none");
        let layout = data_layout(&scratch);
        fs::remove_dir_all(&layout.data).unwrap();
        assert!(!remove_data(&layout, &base(&scratch)).unwrap());
    }

    #[test]
    fn the_dev_profile_inside_prod_is_kept_when_prod_is_deleted() {
        let scratch = Scratch::short("nested");
        let mut layout = data_layout(&scratch);
        let base = scratch.0.join("base");
        layout.data = data_directory(&PROD, Some(base.clone())).unwrap();
        let dev = data_directory(&DEV, Some(base.clone())).unwrap();
        assert!(dev.starts_with(&layout.data), "dev data is inside prod's");
        fill(&layout.data);
        fill(&dev);
        assert!(remove_data(&layout, &base).unwrap());
        assert!(!layout.data.join("soyeht.db").exists());
        assert!(!layout.data.join("logs").exists());
        assert!(
            dev.join("soyeht.db").is_file(),
            "the other profile's data stays"
        );
        assert!(dev.join("logs/engine.log").is_file());
    }

    #[test]
    fn deleting_dev_leaves_prod_alone() {
        let scratch = Scratch::short("dev");
        let mut layout = data_layout(&scratch);
        let base = scratch.0.join("base");
        let prod = data_directory(&PROD, Some(base.clone())).unwrap();
        layout.profile = &DEV;
        layout.data = data_directory(&DEV, Some(base.clone())).unwrap();
        fill(&prod);
        fill(&layout.data);
        assert!(remove_data(&layout, &base).unwrap());
        assert!(!layout.data.exists());
        assert!(prod.join("soyeht.db").is_file());
    }

    #[test]
    fn the_legacy_apps_data_and_unmarked_places_are_refused_and_kept() {
        let scratch = Scratch::short("legacy");
        let mut layout = data_layout(&scratch);
        let legacy = legacy_values::data(&scratch.0);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("keep"), "x").unwrap();
        layout.data = legacy.clone();
        let error = remove_data(&layout, &base(&scratch))
            .unwrap_err()
            .to_string();
        assert!(error.contains("legacy"), "{error}");
        assert!(legacy.join("keep").is_file());

        // The scratch folder itself carries "next", so a place without it is made beside it.
        let unmarked = std::env::temp_dir().join(format!("plain-folder-{}", std::process::id()));
        fs::create_dir_all(&unmarked).unwrap();
        layout.data = unmarked.clone();
        let refused = remove_data(&layout, &base(&scratch));
        let kept = unmarked.is_dir();
        fs::remove_dir_all(&unmarked).unwrap();
        assert!(refused.is_err());
        assert!(kept);
    }

    #[test]
    fn the_home_folder_and_a_relative_or_dotted_place_are_refused() {
        let scratch = Scratch::short("home");
        let mut layout = data_layout(&scratch);
        layout.home = layout.data.join("home");
        fs::create_dir_all(&layout.home).unwrap();
        let error = remove_data(&layout, &base(&scratch))
            .unwrap_err()
            .to_string();
        assert!(error.contains("home folder"), "{error}");
        assert!(layout.home.is_dir());

        let mut layout = data_layout(&scratch);
        layout.data = layout.data.join("..").join("next");
        assert!(remove_data(&layout, &base(&scratch)).is_err());
        layout.data = PathBuf::from("next/relative");
        assert!(remove_data(&layout, &base(&scratch)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_refused_and_a_link_inside_is_removed_without_following_it() {
        let scratch = Scratch::short("link");
        let layout = data_layout(&scratch);
        let outside = scratch.0.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, layout.data.join("inside")).unwrap();
        assert!(remove_data(&layout, &base(&scratch)).unwrap());
        assert!(outside.join("precious").is_file(), "a link is not followed");

        let layout = data_layout(&scratch);
        fs::remove_dir_all(&layout.data).unwrap();
        let target = scratch.0.join("next-target");
        fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, &layout.data).unwrap();
        assert!(matches!(
            remove_data(&layout, &base(&scratch)),
            Err(Error::Unsafe(_))
        ));
        assert!(target.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn data_whose_engine_still_answers_is_kept() {
        let scratch = Scratch::short("running");
        let layout = data_layout(&scratch);
        fill(&layout.data);
        let socket = layout.data.join(ENGINE_SOCKET);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert!(matches!(
            remove_data(&layout, &base(&scratch)),
            Err(Error::StillRunning)
        ));
        assert!(layout.data.join("soyeht.db").is_file());
        drop(listener);
        // A socket file nobody listens on is a stale one, and does not keep the data.
        assert!(remove_data(&layout, &base(&scratch)).unwrap());
    }
}
