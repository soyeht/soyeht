//! Deleting a profile's data: the one thing an uninstall keeps unless the person asks.
//!
//! The data directory holds the database (workspaces, transcripts), the sockets and the logs.
//! Deleting it is irreversible, so it is a plan first ([`plan_removal`], which changes nothing
//! and refuses whatever could be somebody else's) and an execution after ([`Removal::execute`]).
//!
//! The only folder that is ever deleted is the profile's own: the one `contracts/profiles.json`
//! names below the system's folder for application data. A custom `--data-dir`, a folder that
//! merely has "next" in its name (`nextcloud`) and a link into the legacy app's folder are all
//! refused, since every check reads the canonical paths of both sides. A profile's directory can
//! hold another profile's (Dev is inside prod's), and that one is kept.

use crate::{
    error::{Error, Result},
    guard,
    layout::Layout,
};
use soyeht_platform::{fs::canonicalize, local::connect};
use soyeht_protocol::{
    data_directory::data_directory,
    profile::{ENGINE_SOCKET, PROFILES, PTYD_SOCKET},
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How often the endpoints are asked whether the programs have stopped.
const POLL: Duration = Duration::from_millis(100);

/// A deletion that was checked and is ready to run.
#[derive(Debug)]
pub struct Removal {
    /// The canonical path of the profile's data directory.
    data: PathBuf,
    /// The canonical data directories of other profiles inside it, which stay.
    keep: Vec<PathBuf>,
}

fn refuse(reason: &str) -> Error {
    Error::DataRefused(reason.to_owned())
}

/// Checks that the data directory of `layout` may be deleted and returns what would go, or
/// `None` when there is no directory. Nothing is changed, no program is asked and no service is
/// touched, so it runs before anything else does.
///
/// `base` is the system's folder for application data, where the profile's own folder is.
///
/// # Errors
/// [`Error::Legacy`] for a place that is not the app's own or is the legacy app's,
/// [`Error::DataRefused`] when the directory is not the profile's own folder, is a link, or
/// holds the home folder, and [`Error::Io`] when a path cannot be resolved.
pub fn plan_removal(layout: &Layout, base: &Path) -> Result<Option<Removal>> {
    guard::require_path("data directory", &layout.data)?;
    match fs::symlink_metadata(&layout.data) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(refuse("the data directory is a link or not a folder")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let data = canonicalize(&layout.data)?;
    let own = data_directory(layout.profile, Some(base.to_path_buf()))
        .map_err(|_| Error::Environment("the data folder"))?;
    // A folder of the profile that is not there is not this one.
    let own = canonicalize(&own).map_err(|_| refuse("it is not the profile's own folder"))?;
    if data != own {
        return Err(refuse(
            "it is not the profile's own folder; only that folder is deleted",
        ));
    }
    guard::require_path("data directory", &data)?;
    let home = canonicalize(&layout.home).map_err(|_| Error::Environment("the home folder"))?;
    if home.starts_with(&data) {
        return Err(refuse("it holds the home folder"));
    }
    let mut keep = Vec::new();
    for profile in PROFILES.iter().filter(|p| p.name != layout.profile.name) {
        let other = data_directory(profile, Some(base.to_path_buf()))
            .map_err(|_| Error::Environment("the data folder"))?;
        match canonicalize(&other) {
            Ok(other) if other.starts_with(&data) && other != data => keep.push(other),
            Ok(_) => {}
            // A profile with no data has nothing to keep.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Some(Removal { data, keep }))
}

impl Removal {
    /// Deletes what the plan names, once the engine and the supervisor have stopped answering
    /// on their endpoints. A program that was just unregistered takes a moment to end, so this
    /// waits up to `wait` for it.
    ///
    /// # Errors
    /// [`Error::StillRunning`] when a program still answers after `wait`, and [`Error::Io`] when
    /// a file cannot be removed.
    pub fn execute(&self, wait: Duration) -> Result<()> {
        let deadline = Instant::now() + wait;
        while self.answers() {
            if Instant::now() >= deadline {
                return Err(Error::StillRunning);
            }
            std::thread::sleep(POLL);
        }
        remove_except(&self.data, &self.keep)?;
        Ok(())
    }

    fn answers(&self) -> bool {
        [ENGINE_SOCKET, PTYD_SOCKET]
            .iter()
            .any(|socket| connect(&self.data.join(socket)).is_ok())
    }
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
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        remove_link(path, &meta)
    } else if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// A link to a folder is removed like a folder on Windows (`RemoveDirectory`), and like a file
/// elsewhere.
#[cfg(windows)]
fn remove_link(path: &Path, meta: &fs::Metadata) -> io::Result<()> {
    use std::os::windows::fs::FileTypeExt as _;
    if meta.file_type().is_symlink_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(not(windows))]
fn remove_link(path: &Path, _: &fs::Metadata) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{guard::legacy_values, layout::test_layout, system::System, testing::Scratch};
    use soyeht_protocol::profile::{DEV, PROD};

    fn base(scratch: &Scratch) -> PathBuf {
        scratch.0.join("base")
    }

    /// The layout of prod with its data where the profile keeps it below the scratch folder's
    /// `base`, made, and a home folder beside it.
    fn data_layout(scratch: &Scratch) -> Layout {
        let mut layout = test_layout(System::current(), &scratch.0);
        layout.data = data_directory(&PROD, Some(base(scratch))).unwrap();
        fs::create_dir_all(&layout.data).unwrap();
        fs::create_dir_all(&layout.home).unwrap();
        layout
    }

    fn fill(data: &Path) {
        fs::create_dir_all(data.join("logs")).unwrap();
        fs::write(data.join("logs/engine.log"), "line").unwrap();
        fs::write(data.join("soyeht.db"), "rows").unwrap();
    }

    /// Plans and runs, waiting for nothing; whether there was a directory to delete.
    fn remove(layout: &Layout, base: &Path) -> Result<bool> {
        match plan_removal(layout, base)? {
            Some(plan) => plan.execute(Duration::ZERO).map(|()| true),
            None => Ok(false),
        }
    }

    fn refused(result: Result<bool>) -> String {
        result.unwrap_err().to_string()
    }

    #[test]
    fn the_data_directory_is_deleted_with_everything_in_it() {
        let scratch = Scratch::short("all");
        let layout = data_layout(&scratch);
        fill(&layout.data);
        assert!(remove(&layout, &base(&scratch)).unwrap());
        assert!(!layout.data.exists());
        // The folders around it are not the seam's to remove.
        assert!(layout.data.parent().unwrap().is_dir());
    }

    #[test]
    fn a_plan_changes_nothing() {
        let scratch = Scratch::short("plan");
        let layout = data_layout(&scratch);
        fill(&layout.data);
        assert!(plan_removal(&layout, &base(&scratch)).unwrap().is_some());
        assert!(layout.data.join("soyeht.db").is_file());
    }

    #[test]
    fn a_directory_that_is_not_there_is_nothing_to_delete() {
        let scratch = Scratch::short("none");
        let layout = data_layout(&scratch);
        fs::remove_dir_all(&layout.data).unwrap();
        assert!(!remove(&layout, &base(&scratch)).unwrap());
    }

    #[test]
    fn the_dev_profile_inside_prod_is_kept_when_prod_is_deleted() {
        let scratch = Scratch::short("nested");
        let layout = data_layout(&scratch);
        let dev = data_directory(&DEV, Some(base(&scratch))).unwrap();
        assert!(dev.starts_with(&layout.data), "dev data is inside prod's");
        fill(&layout.data);
        fill(&dev);
        assert!(remove(&layout, &base(&scratch)).unwrap());
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
        let prod = layout.data.clone();
        layout.profile = &DEV;
        layout.data = data_directory(&DEV, Some(base(&scratch))).unwrap();
        fs::create_dir_all(&layout.data).unwrap();
        fill(&prod);
        fill(&layout.data);
        assert!(remove(&layout, &base(&scratch)).unwrap());
        assert!(!layout.data.exists());
        assert!(prod.join("soyeht.db").is_file());
    }

    #[test]
    fn only_the_profiles_own_folder_is_deleted_whatever_its_name_holds() {
        let scratch = Scratch::short("own");
        let mut layout = data_layout(&scratch);
        let places = [
            base(&scratch).join("nextcloud"),
            scratch.0.join("home/Documents/nextjs-app"),
            scratch.0.join("Users/nextdoor/Documents"),
            scratch.0.join("next"),
            layout.data.join("logs"),
        ];
        for place in places {
            fs::create_dir_all(&place).unwrap();
            fs::write(place.join("mine"), "x").unwrap();
            layout.data = place.clone();
            let message = refused(remove(&layout, &base(&scratch)));
            assert!(
                message.contains("nothing was deleted"),
                "{place:?}: {message}"
            );
            assert!(place.join("mine").is_file(), "{place:?} was kept");
        }
    }

    #[test]
    fn a_place_without_next_or_the_legacy_apps_data_is_refused() {
        let scratch = Scratch::short("legacy");
        let mut layout = data_layout(&scratch);
        let legacy = legacy_values::data(&scratch.0);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("keep"), "x").unwrap();
        layout.data = legacy.clone();
        let message = refused(remove(&layout, &base(&scratch)));
        assert!(message.contains("legacy"), "{message}");
        assert!(legacy.join("keep").is_file());

        // The scratch folder itself carries "next", so a place without it is made beside it.
        let plain = std::env::temp_dir().join(format!("plain-folder-{}", std::process::id()));
        fs::create_dir_all(&plain).unwrap();
        layout.data = plain.clone();
        let result = remove(&layout, &base(&scratch));
        let kept = plain.is_dir();
        fs::remove_dir_all(&plain).unwrap();
        assert!(result.is_err());
        assert!(kept);
    }

    #[test]
    fn the_home_folder_and_a_relative_or_dotted_place_are_refused() {
        let scratch = Scratch::short("home");
        let mut layout = data_layout(&scratch);
        layout.home = layout.data.join("home");
        fs::create_dir_all(&layout.home).unwrap();
        let message = refused(remove(&layout, &base(&scratch)));
        assert!(message.contains("home folder"), "{message}");
        assert!(layout.home.is_dir());

        let mut layout = data_layout(&scratch);
        layout.data = layout.data.join("..").join("next");
        assert!(remove(&layout, &base(&scratch)).is_err());
        layout.data = PathBuf::from("next/relative");
        assert!(remove(&layout, &base(&scratch)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_home_spelled_through_a_link_is_still_the_home() {
        let scratch = Scratch::short("homelink");
        let mut layout = data_layout(&scratch);
        let inside = layout.data.join("person");
        fs::create_dir_all(&inside).unwrap();
        layout.home = scratch.0.join("alias");
        std::os::unix::fs::symlink(&inside, &layout.home).unwrap();
        let message = refused(remove(&layout, &base(&scratch)));
        assert!(message.contains("home folder"), "{message}");
        assert!(inside.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_path_cannot_lead_to_the_legacy_apps_data() {
        let scratch = Scratch::short("link");
        let mut layout = data_layout(&scratch);
        let legacy = legacy_values::data(&scratch.0);
        fs::create_dir_all(legacy.join("sub")).unwrap();
        fs::write(legacy.join("sub/keep"), "x").unwrap();
        let link = scratch.0.join("next-link");
        std::os::unix::fs::symlink(&legacy, &link).unwrap();
        for place in [link.join("sub"), link.clone()] {
            layout.data = place;
            assert!(remove(&layout, &base(&scratch)).is_err());
            assert!(legacy.join("sub/keep").is_file());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_reached_through_a_link_is_the_same_folder_and_a_link_inside_is_not_followed() {
        let scratch = Scratch::short("alias");
        let mut layout = data_layout(&scratch);
        fill(&layout.data);
        let outside = scratch.0.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, layout.data.join("inside")).unwrap();
        // The profile's folder named through another spelling of its parent.
        let alias = scratch.0.join("alias-base");
        std::os::unix::fs::symlink(base(&scratch), &alias).unwrap();
        layout.data = data_directory(&PROD, Some(alias)).unwrap();
        assert!(remove(&layout, &base(&scratch)).unwrap());
        assert!(
            !data_directory(&PROD, Some(base(&scratch)))
                .unwrap()
                .exists()
        );
        assert!(outside.join("precious").is_file(), "a link is not followed");
    }

    #[cfg(unix)]
    #[test]
    fn a_data_directory_that_is_itself_a_link_is_refused() {
        let scratch = Scratch::short("toplink");
        let layout = data_layout(&scratch);
        let moved = scratch.0.join("moved");
        fs::rename(&layout.data, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &layout.data).unwrap();
        assert!(remove(&layout, &base(&scratch)).is_err());
        assert!(moved.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn data_whose_engine_still_answers_is_kept_until_it_stops() {
        let scratch = Scratch::short("running");
        let layout = data_layout(&scratch);
        fill(&layout.data);
        let socket = layout.data.join(ENGINE_SOCKET);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let plan = plan_removal(&layout, &base(&scratch)).unwrap().unwrap();
        assert!(matches!(
            plan.execute(Duration::ZERO),
            Err(Error::StillRunning)
        ));
        assert!(layout.data.join("soyeht.db").is_file());
        // A program that is ending is waited for.
        let ending = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(listener);
        });
        plan.execute(Duration::from_secs(10)).unwrap();
        ending.join().unwrap();
        assert!(!layout.data.exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_link_to_a_folder_inside_is_removed_without_following_it() {
        let scratch = Scratch::short("winlink");
        let layout = data_layout(&scratch);
        let outside = scratch.0.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious"), "x").unwrap();
        // A link needs a privilege or developer mode; without one there is nothing to test.
        if std::os::windows::fs::symlink_dir(&outside, layout.data.join("inside")).is_err() {
            return;
        }
        assert!(remove(&layout, &base(&scratch)).unwrap());
        assert!(outside.join("precious").is_file());
    }
}
