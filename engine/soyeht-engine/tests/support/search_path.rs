//! A rig whose engine does not ask the machine's login shell where programs are.

use super::*;

impl Rig {
    /// The engine's own `PATH`, which it falls back to when its login shell does not answer in
    /// time. A rig with a shell of its own keeps the machine's folders out of it, as it does out
    /// of the shell's answer, so a busy host cannot make the engine find the machine's programs.
    pub fn engine_path(&self) -> std::ffi::OsString {
        if self.login_shell.is_none() {
            return test_path(&self.path);
        }
        std::env::join_paths([self.path.join("bin"), "/usr/bin".into(), "/bin".into()]).unwrap()
    }

    /// A rig whose engine searches the rig's `bin` and the system's, and nothing else. The machine's
    /// own login shell is not asked, so a `claude` or an `npx` it installs cannot stand in for the
    /// rig's fakes.
    #[cfg(unix)]
    pub fn with_own_search_path() -> Self {
        let mut rig = Self::new();
        let shell_directory = rig.path.join("shell");
        fs::create_dir(&shell_directory).unwrap();
        write_script(
            &shell_directory.join("bash"),
            &format!(
                "#!/bin/sh\nprintf '\\036%s\\037' '{}:/usr/bin:/bin'\n",
                rig.path.join("bin").display()
            ),
        );
        rig.kill_engine();
        // The first engine may have logged its own choice before it was stopped; a test counts
        // only the choices of the engine that searches the rig's path. It also remembered the
        // launch `PATH` of this machine in the data directory, and an engine uses what it
        // remembers until its shell answers: left there, the engine that starts below would find
        // the machine's own `claude` and log it before the rig's shell answered, on a busy host.
        for stale in ["engine.log", "launch-path"] {
            if let Err(error) = fs::remove_file(rig.path.join(stale)) {
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
            }
        }
        rig.login_shell = Some(shell_directory.join("bash"));
        rig.start();
        rig
    }
}
