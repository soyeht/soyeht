//! A command that leads its own process group, which is killed as a whole.

use crate::Streams;
use soyeht_platform::process::kill_tree;
use std::{
    fs::File,
    io,
    os::{fd::OwnedFd, unix::process::CommandExt},
    process::{Child, Command, ExitStatus, Stdio},
};

pub(crate) struct Running(Child);

impl Running {
    /// Starts `command` with both output streams piped, and standard input piped when the
    /// caller has `input` to send, closed otherwise.
    pub(crate) fn spawn(mut command: Command, input: bool) -> io::Result<Self> {
        command
            .stdin(if input { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        command.spawn().map(Self)
    }

    /// The command's pipes, once.
    pub(crate) fn take_streams(&mut self) -> Streams {
        let file = |fd: OwnedFd| File::from(fd);
        Streams {
            input: self.0.stdin.take().map(OwnedFd::from).map(file),
            output: self.0.stdout.take().map(OwnedFd::from).map(file),
            error: self.0.stderr.take().map(OwnedFd::from).map(file),
        }
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.0.try_wait()
    }

    /// Ends the command with everything it started and reaps it.
    pub(crate) fn kill(&mut self) {
        kill_tree(self.0.id());
        // Best effort: the caller reports the timeout, whatever happens to the child here.
        // kill fails only when the child already exited, which is the goal.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }

    /// The command finished by itself: what it left running is not ours to end.
    pub(crate) fn release(&self) {}
}
