//! Whether `git` can run, decided without running it.
//!
//! On a stock Mac, `/usr/bin/git` is a launcher: when the Command Line Tools are missing it
//! opens the system installer instead of running Git. `command -v git` therefore proves
//! nothing, and starting the program to find out is the very thing to avoid. [`Probe`] looks
//! at the files the launcher itself consults and reports [`Availability`] without starting a
//! process. Every other system reports [`Availability::Available`]: a missing `git` there is
//! a plain spawn failure with no dialog.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

/// Where the active developer directory is recorded by `xcode-select`.
#[cfg(target_os = "macos")]
const SELECT_LINK: &str = "/var/db/xcode_select_link";
/// Where the Command Line Tools install themselves.
#[cfg(target_os = "macos")]
const DEFAULT_TOOLS: &str = "/Library/Developer/CommandLineTools";
/// The launcher that opens the installer.
#[cfg(target_os = "macos")]
const LAUNCHER: &str = "/usr/bin/git";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// `git` would open the system installer, or there is no `git` to start.
    Unavailable,
}

/// The files and variables that decide whether `git` runs.
#[derive(Debug, Clone)]
pub struct Probe {
    /// The launcher that needs a developer directory behind it. `None` where no such
    /// launcher exists: every `git` found on the path is then taken as it is.
    pub launcher: Option<PathBuf>,
    /// The search path the program is started with.
    pub path: OsString,
    /// `DEVELOPER_DIR`, which overrides the recorded developer directory.
    pub developer_dir: Option<PathBuf>,
    /// The symbolic link `xcode-select --switch` writes.
    pub select_link: PathBuf,
    /// The directory the Command Line Tools install into.
    pub default_tools: PathBuf,
}

impl Probe {
    /// The probe of this process: its `PATH` and `DEVELOPER_DIR`, and the system locations.
    pub fn system() -> Self {
        Self {
            launcher: system_launcher(),
            path: std::env::var_os("PATH").unwrap_or_default(),
            developer_dir: std::env::var_os("DEVELOPER_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            select_link: system_select_link(),
            default_tools: system_default_tools(),
        }
    }

    pub fn availability(&self) -> Availability {
        let Some(launcher) = &self.launcher else {
            return Availability::Available;
        };
        let Some(first) = self.first_git() else {
            return Availability::Unavailable;
        };
        if !is_launcher(&first, launcher) || self.developer_tools_present() {
            Availability::Available
        } else {
            Availability::Unavailable
        }
    }

    /// The `git` the system would start: the first executable one on the path.
    fn first_git(&self) -> Option<PathBuf> {
        std::env::split_paths(&self.path)
            .map(|directory| directory.join("git"))
            .find(|candidate| is_executable(candidate))
    }

    /// The directory the launcher consults, in its order, holds a `git`.
    fn developer_tools_present(&self) -> bool {
        let directory = match &self.developer_dir {
            Some(directory) => directory.clone(),
            None => fs::read_link(&self.select_link).unwrap_or_else(|_| self.default_tools.clone()),
        };
        is_executable(&directory.join("usr/bin/git"))
    }
}

fn is_launcher(candidate: &Path, launcher: &Path) -> bool {
    candidate == launcher
        || matches!(
            (fs::canonicalize(candidate), fs::canonicalize(launcher)),
            (Ok(left), Ok(right)) if left == right
        )
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(target_os = "macos")]
fn system_launcher() -> Option<PathBuf> {
    Some(PathBuf::from(LAUNCHER))
}

#[cfg(not(target_os = "macos"))]
fn system_launcher() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
fn system_select_link() -> PathBuf {
    PathBuf::from(SELECT_LINK)
}

#[cfg(not(target_os = "macos"))]
fn system_select_link() -> PathBuf {
    PathBuf::new()
}

#[cfg(target_os = "macos")]
fn system_default_tools() -> PathBuf {
    PathBuf::from(DEFAULT_TOOLS)
}

#[cfg(not(target_os = "macos"))]
fn system_default_tools() -> PathBuf {
    PathBuf::new()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    struct Machine {
        root: PathBuf,
    }

    impl Machine {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("soyeht-git-probe-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("usr/bin")).unwrap();
            Self { root }
        }

        fn program(&self, relative: &str) -> PathBuf {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn probe(&self, path: &[&str]) -> Probe {
            Probe {
                launcher: Some(self.root.join("usr/bin/git")),
                path: std::env::join_paths(path.iter().map(|part| self.root.join(part))).unwrap(),
                developer_dir: None,
                select_link: self.root.join("var/xcode_select_link"),
                default_tools: self.root.join("Library/CommandLineTools"),
            }
        }
    }

    impl Drop for Machine {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn launcher_without_developer_tools_is_unavailable() {
        let machine = Machine::new("bare");
        machine.program("usr/bin/git");
        assert_eq!(
            machine.probe(&["usr/bin"]).availability(),
            Availability::Unavailable
        );
    }

    #[test]
    fn default_command_line_tools_make_the_launcher_available() {
        let machine = Machine::new("default");
        machine.program("usr/bin/git");
        machine.program("Library/CommandLineTools/usr/bin/git");
        assert_eq!(
            machine.probe(&["usr/bin"]).availability(),
            Availability::Available
        );
    }

    #[test]
    fn the_recorded_developer_directory_wins_over_the_default() {
        let machine = Machine::new("link");
        machine.program("usr/bin/git");
        machine.program("Library/CommandLineTools/usr/bin/git");
        fs::create_dir_all(machine.root.join("var")).unwrap();
        symlink(
            machine.root.join("Xcode/Developer"),
            machine.root.join("var/xcode_select_link"),
        )
        .unwrap();
        assert_eq!(
            machine.probe(&["usr/bin"]).availability(),
            Availability::Unavailable
        );
        machine.program("Xcode/Developer/usr/bin/git");
        assert_eq!(
            machine.probe(&["usr/bin"]).availability(),
            Availability::Available
        );
    }

    #[test]
    fn developer_dir_overrides_the_recorded_directory() {
        let machine = Machine::new("env");
        machine.program("usr/bin/git");
        machine.program("custom/usr/bin/git");
        let mut probe = machine.probe(&["usr/bin"]);
        probe.developer_dir = Some(machine.root.join("custom"));
        assert_eq!(probe.availability(), Availability::Available);
        probe.developer_dir = Some(machine.root.join("missing"));
        assert_eq!(probe.availability(), Availability::Unavailable);
    }

    #[test]
    fn a_git_that_is_not_the_launcher_needs_no_developer_tools() {
        let machine = Machine::new("brew");
        machine.program("usr/bin/git");
        machine.program("opt/bin/git");
        assert_eq!(
            machine.probe(&["opt/bin", "usr/bin"]).availability(),
            Availability::Available
        );
    }

    #[test]
    fn the_launcher_first_on_the_path_decides_even_with_another_git_behind_it() {
        let machine = Machine::new("order");
        machine.program("usr/bin/git");
        machine.program("opt/bin/git");
        assert_eq!(
            machine.probe(&["usr/bin", "opt/bin"]).availability(),
            Availability::Unavailable
        );
    }

    #[test]
    fn a_link_to_the_launcher_is_the_launcher() {
        let machine = Machine::new("alias");
        machine.program("usr/bin/git");
        fs::create_dir_all(machine.root.join("opt/bin")).unwrap();
        symlink(
            machine.root.join("usr/bin/git"),
            machine.root.join("opt/bin/git"),
        )
        .unwrap();
        assert_eq!(
            machine.probe(&["opt/bin"]).availability(),
            Availability::Unavailable
        );
    }

    #[test]
    fn no_git_on_the_path_is_unavailable_where_a_launcher_exists() {
        let machine = Machine::new("none");
        assert_eq!(
            machine.probe(&["usr/bin"]).availability(),
            Availability::Unavailable
        );
    }

    #[test]
    fn a_system_without_a_launcher_is_never_probed() {
        let machine = Machine::new("other");
        let mut probe = machine.probe(&["usr/bin"]);
        probe.launcher = None;
        assert_eq!(probe.availability(), Availability::Available);
    }
}
