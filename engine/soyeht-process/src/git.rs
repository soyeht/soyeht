//! Starting `git` without opening the macOS installer. See [`soyeht_platform::git`].

use crate::{Error, Limits, Output, Result, run};
use soyeht_platform::git::{Availability, Probe};
use std::process::Command;

/// What a person is told when `git` is not started. It names the cause and the way out, and
/// says Soyeht did not open the installer.
pub const GIT_UNAVAILABLE_MESSAGE: &str = "Git is not available on this computer. Soyeht did not start it, because on a Mac without the Command Line Tools that opens the system installer. Install Git or the Command Line Tools when you choose, then try again.";
/// Fails with [`Error::GitUnavailable`] when `git` cannot run on this machine, without
/// starting it. On a Mac without the Command Line Tools, starting `/usr/bin/git` opens the
/// system installer, so every path that starts `git`, or a program that starts it, calls this
/// first (or uses [`run_git`]).
pub fn require_git() -> Result<()> {
    require_git_with(&Probe::system())
}

pub fn require_git_with(probe: &Probe) -> Result<()> {
    match probe.availability() {
        Availability::Available => Ok(()),
        Availability::Unavailable => Err(Error::GitUnavailable),
    }
}

/// [`run`] for a `git` command, after [`require_git`].
pub fn run_git(command: Command, limits: Limits) -> Result<Output> {
    run_git_with(&Probe::system(), command, limits)
}

pub fn run_git_with(probe: &Probe, command: Command, limits: Limits) -> Result<Output> {
    require_git_with(probe)?;
    run(command, limits)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{env, fs, path::PathBuf, time::Duration};

    /// A machine whose `git` is a launcher that records that it ran. The launcher stands for
    /// the macOS stub that opens the installer.
    struct Launcher {
        root: PathBuf,
    }

    impl Launcher {
        fn new(label: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let root =
                env::temp_dir().join(format!("soyeht-run-git-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("usr/bin")).unwrap();
            let script = format!("#!/bin/sh\ntouch '{}'\n", root.join("ran").display());
            let launcher = root.join("usr/bin/git");
            fs::write(&launcher, script).unwrap();
            fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
            Self { root }
        }

        fn probe(&self) -> Probe {
            Probe {
                launcher: Some(self.root.join("usr/bin/git")),
                path: self.root.join("usr/bin").into_os_string(),
                developer_dir: None,
                select_link: self.root.join("xcode_select_link"),
                default_tools: self.root.join("Library/CommandLineTools"),
            }
        }

        fn ran(&self) -> bool {
            self.root.join("ran").exists()
        }
    }

    impl Drop for Launcher {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn git_is_not_started_when_the_developer_tools_are_missing() {
        let machine = Launcher::new("missing");
        let limits = Limits::new(Duration::from_secs(10), 8192);
        let error = run_git_with(&machine.probe(), Command::new("git"), limits).unwrap_err();
        assert!(matches!(error, Error::GitUnavailable), "{error}");
        assert!(matches!(
            require_git_with(&machine.probe()),
            Err(Error::GitUnavailable)
        ));
        assert!(!machine.ran(), "the launcher must never start");
    }

    #[test]
    fn git_starts_when_the_developer_tools_are_present() {
        use std::os::unix::fs::PermissionsExt;
        let machine = Launcher::new("present");
        let real = machine.root.join("Library/CommandLineTools/usr/bin/git");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = Command::new(machine.root.join("usr/bin/git"));
        command.arg("--version");
        let limits = Limits::new(Duration::from_secs(10), 8192);
        run_git_with(&machine.probe(), command, limits).unwrap();
        assert!(machine.ran());
    }
}
