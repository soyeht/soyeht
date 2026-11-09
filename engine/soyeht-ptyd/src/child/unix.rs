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

    /// A handle that blocks until this child has ended, without holding the child itself, so the
    /// kill and close paths can take the child's lock while it waits.
    pub fn exit_waiter(&self) -> ExitWaiter {
        ExitWaiter { pid: self.0.id() }
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

/// Waits for a child to end. It leaves the child to be collected by [`Child::try_wait`], so the
/// process id cannot be reused by another process before the owner has seen the exit status.
pub struct ExitWaiter {
    pid: u32,
}

impl ExitWaiter {
    /// Blocks until the child has ended or has already been collected by its owner.
    pub fn wait(&self) -> io::Result<()> {
        loop {
            // SAFETY: the structure is plain data and zero is a valid value for every field.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: `info` is an out pointer of the type the call fills. WNOWAIT leaves the
            // ended child for its owner to collect.
            let waited = unsafe {
                libc::waitid(
                    libc::P_PID,
                    libc::id_t::from(self.pid),
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            };
            if waited == 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => {}
                // The owner collected the child first, which is also an ended child.
                Some(libc::ECHILD) => return Ok(()),
                _ => return Err(error),
            }
        }
    }
}
