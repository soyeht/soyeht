use super::Exit;
use soyeht_platform::process::{GroupSignal, signal_group};
use std::io;

/// A child that leads its own process group (a terminal's session, or a pipe guardian).
pub struct Child(std::process::Child);

impl Child {
    pub(crate) fn new(inner: std::process::Child) -> Self {
        Self(inner)
    }

    pub fn id(&self) -> u32 {
        self.0.id()
    }

    /// The child's standard input and output, when it was started with pipes.
    pub(crate) fn take_pipes(
        &mut self,
    ) -> (
        Option<std::process::ChildStdin>,
        Option<std::process::ChildStdout>,
    ) {
        (self.0.stdin.take(), self.0.stdout.take())
    }

    pub fn try_wait(&mut self) -> io::Result<Option<Exit>> {
        Ok(self.0.try_wait()?.map(|status| Exit(status.code())))
    }

    pub fn wait(&mut self) -> io::Result<Exit> {
        Ok(Exit(self.0.wait()?.code()))
    }

    /// Ends the child's process group. A process a person detached from the shell with its own
    /// session stays.
    pub fn kill_group(&self) {
        signal_group(self.0.id(), GroupSignal::Kill);
    }

    /// Asks the child's process group to stop, as a terminal being closed does.
    pub fn request_stop(&self) {
        signal_group(self.0.id(), GroupSignal::Terminate);
    }

    /// Ends a guarded (agent or language server) process and everything below it, including
    /// children that left its process group.
    pub fn kill_tree(&self) {
        // Snapshot first: once the leader dies its children are reparented and no longer
        // reachable from it.
        let escaped = crate::tree::descendants(self.0.id());
        self.kill_group();
        for descendant in escaped {
            if let Ok(descendant) = i32::try_from(descendant)
                && descendant > 1
            {
                // SAFETY: the pid was read from the process table an instant ago, as a
                // descendant of the instance this supervisor spawned.
                unsafe {
                    libc::kill(descendant, libc::SIGKILL);
                }
            }
        }
    }
}
