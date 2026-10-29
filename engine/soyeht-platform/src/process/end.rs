//! Ending a child process: kill it if it still runs, then reap it, within a bound.
//!
//! The same on every system, because it only uses what `std::process::Child` offers. To reach
//! what the child started as well, signal its group first ([`super::kill_tree`] on Unix) or end
//! its job (Windows); this step then reaps the child itself.

use std::{
    io,
    process::Child,
    thread,
    time::{Duration, Instant},
};

use crate::log;

const POLL: Duration = Duration::from_millis(5);
/// How long ending a child waits for the system to report that it ended. A killed child is
/// gone in milliseconds; the bound only keeps one that is stuck in the kernel from holding the
/// caller for good.
const END_WAIT: Duration = Duration::from_secs(5);

/// How [`end_child`] left the child. It is logged when it is a failure; nothing else reads it
/// but the tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildEnd {
    /// The child had already ended; it is reaped now.
    AlreadyExited,
    /// The child was running or just ended; it is killed and reaped.
    Killed,
    /// The child was killed but its exit was not seen within [`END_WAIT`]. It stays a zombie
    /// until this process reaps it or ends.
    Unreaped,
    /// The system refused to kill or to wait. The failure is logged by kind.
    Failed,
}

/// Ends `child`: kills it when it still runs, then waits at most five seconds for the system to
/// report that it ended. A child that cannot be killed or reaped is logged by kind
/// (`process.end_child_failed`, `process.end_child_unreaped`); there is nothing else for a
/// caller to do with such a child.
pub fn end_child(child: &mut Child) {
    ended(child);
}

fn ended(child: &mut Child) -> ChildEnd {
    match child.try_wait() {
        Ok(Some(_)) => return ChildEnd::AlreadyExited,
        Ok(None) => {}
        Err(error) => return failed(&error),
    }
    // A child that ends between the check above and this call makes the kill fail with
    // `InvalidInput` on older systems; the wait below reports its exit either way.
    if let Err(error) = child.kill()
        && error.kind() != io::ErrorKind::InvalidInput
    {
        return failed(&error);
    }
    let deadline = Instant::now() + END_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return ChildEnd::Killed,
            Ok(None) => {}
            Err(error) => return failed(&error),
        }
        if Instant::now() >= deadline {
            log::event("process.end_child_unreaped", &[]);
            return ChildEnd::Unreaped;
        }
        thread::sleep(POLL);
    }
}

fn failed(error: &io::Error) -> ChildEnd {
    log::failure("process.end_child_failed", error);
    ChildEnd::Failed
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    #[cfg(unix)]
    fn command(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[cfg(windows)]
    fn command(script: &str) -> Command {
        let mut command = Command::new("cmd");
        command.args(["/c", script]);
        command
    }

    fn long_running() -> Child {
        #[cfg(unix)]
        let script = "sleep 60";
        #[cfg(windows)]
        let script = "ping -n 60 127.0.0.1 >nul";
        command(script).stdout(Stdio::null()).spawn().unwrap()
    }

    #[test]
    fn a_running_child_is_killed_and_reaped() {
        let _forks = crate::spawning();
        let mut child = long_running();
        assert_eq!(ended(&mut child), ChildEnd::Killed);
        // Reaped: the status is already known and a second wait returns it.
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn a_child_that_already_exited_is_reported_as_such() {
        let _forks = crate::spawning();
        let mut child = command("exit 0").spawn().unwrap();
        child.wait().unwrap();
        assert_eq!(ended(&mut child), ChildEnd::AlreadyExited);
        // Ending it again changes nothing.
        assert_eq!(ended(&mut child), ChildEnd::AlreadyExited);
    }
}
