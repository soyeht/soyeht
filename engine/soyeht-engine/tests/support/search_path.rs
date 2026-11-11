//! A rig whose engine does not ask the machine's login shell where programs are.

use super::*;

impl Rig {
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
        // only the choices of the engine that searches the rig's path.
        if let Err(error) = fs::remove_file(rig.path.join("engine.log")) {
            assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
        }
        rig.login_shell = Some(shell_directory.join("bash"));
        rig.start();
        rig
    }
}
