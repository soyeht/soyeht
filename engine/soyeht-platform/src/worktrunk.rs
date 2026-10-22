//! The name of the worktrunk program the engine runs for task worktrees.
//!
//! The Windows package installs it as `git-wt`: `wt` there is Windows Terminal, which starting
//! it would open a window. Everything that looks the program up or runs it uses [`PROGRAM`].

/// The command that starts worktrunk on this system.
pub const PROGRAM: &str = if cfg!(windows) { "git-wt" } else { "wt" };

#[cfg(test)]
mod tests {
    use super::PROGRAM;

    #[test]
    fn windows_never_starts_windows_terminal() {
        assert_eq!(PROGRAM, if cfg!(windows) { "git-wt" } else { "wt" });
    }
}
