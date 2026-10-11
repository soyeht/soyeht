use super::{Exit, Reach};
use soyeht_platform::process::{GroupSignal, kill_tree, signal_group};
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

    /// Ends the child's processes as far as `reach` says.
    pub fn kill(&self, reach: Reach) {
        match reach {
            Reach::Group => signal_group(self.0.id(), GroupSignal::Kill),
            Reach::Tree => kill_tree(self.0.id()),
        }
    }

    /// Asks the child's process group to stop, as a terminal being closed does.
    pub fn request_stop(&self) {
        signal_group(self.0.id(), GroupSignal::Terminate);
    }
}
