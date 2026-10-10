//! A command that runs in its own job object, which is terminated as a whole.

use crate::Streams;
use soyeht_platform::process::windows::{self as winproc, Job, Launch, Spawned, Stdio};
use std::{
    fs::File,
    io,
    os::windows::{io::OwnedHandle, process::ExitStatusExt},
    process::{Command, ExitStatus},
    time::Duration,
};

/// How long a killed command may take to be gone before it is left to the system.
const END_WAIT: Duration = Duration::from_secs(5);

pub(crate) struct Running {
    spawned: Spawned,
    streams: Option<Streams>,
}

/// Reports a failure that nothing in the caller's result carries, as one structured line.
fn report(event: &str, error: &io::Error) {
    eprintln!(
        "{{\"event\":\"process.{event}\",\"error\":\"{:?}\"}}",
        error.kind()
    );
}

impl Running {
    /// Starts `command` inside a job with both output streams piped, and standard input piped
    /// when the caller has `input` to send, closed otherwise. The command gets this process's
    /// environment with its own variables applied over it.
    pub(crate) fn spawn(command: Command, input: bool) -> io::Result<Self> {
        let (output_read, output_write) = winproc::pipe()?;
        let (error_read, error_write) = winproc::pipe()?;
        let (input_read, input_write) = if input {
            let (read, write) = winproc::pipe()?;
            (read, Some(write))
        } else {
            (OwnedHandle::from(File::open("NUL")?), None)
        };
        let job = Job::new()?;
        // The job is the group: it holds the command from its first instruction, so nothing it
        // starts can escape before the job knows it.
        let (process, _) = winproc::create_process(&Launch {
            command: &command,
            job: &job,
            console: None,
            stdio: Some(Stdio {
                input: &input_read,
                output: &output_write,
                error: &error_write,
            }),
            inherit_environment: true,
        })?;
        // Only the command holds the write ends now, so the streams end when it and its
        // children end.
        drop((input_read, output_write, error_write));
        Ok(Self {
            spawned: Spawned::new(process, job),
            streams: Some(Streams {
                input: input_write.map(File::from),
                output: Some(File::from(output_read)),
                error: Some(File::from(error_read)),
            }),
        })
    }

    /// The command's pipes, once.
    pub(crate) fn take_streams(&mut self) -> Streams {
        self.streams.take().unwrap_or(Streams {
            input: None,
            output: None,
            error: None,
        })
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(self.spawned.wait_exit(0)?.map(ExitStatus::from_raw))
    }

    /// Ends the command with everything it started and waits for it to be gone. The caller
    /// reports the timeout, so a failure here is only logged.
    pub(crate) fn kill(&mut self) {
        if let Err(error) = self.spawned.kill_tree(END_WAIT) {
            report("kill_tree_failed", &error);
        }
        if let Err(error) = self.spawned.wait_exit(0) {
            report("wait_failed", &error);
        }
    }

    /// The command finished by itself: what it left running is not ours to end, as on Unix.
    pub(crate) fn release(&self) {
        // If the job keeps its kill-on-close limit, a leftover process ends with it, which
        // is the safe outcome; the caller's result does not depend on it.
        if let Err(error) = self.spawned.release() {
            report("release_failed", &error);
        }
    }
}
