//! On Windows the guard is a job: the system ends everything in it when the supervisor's last
//! handle to the job closes, which is when the supervisor ends, however it ends.

use crate::{
    Error, Result,
    child::Child,
    io::{Output, input_without_waiting},
    process::{self, Spawned},
    wire::Spawn,
};
use soyeht_platform::process::windows::{self as winproc, Job, Launch, Stdio};
use std::fs::File;
use std::path::Path;

/// A pipe process has no guardian to ask for its exit code; the process handle has it.
pub(crate) struct Guard;

impl Guard {
    pub(crate) fn exit_code(&mut self) -> Option<i32> {
        None
    }
}

pub(crate) fn spawn(config: &Spawn, directory: &Path, id: &str) -> Result<Spawned> {
    let command = process::command(config)?;
    let stderr = super::open_diagnostic_log(directory, id).map_err(Error::guardian_setup)?;
    let (child_input, input_write) = winproc::pipe()?;
    let (output_read, child_output) = winproc::pipe()?;
    let stderr = std::os::windows::io::OwnedHandle::from(stderr);
    let job = Job::new().map_err(Error::guardian_setup)?;
    let (process, pid) = winproc::create_process(&Launch {
        job: &job,
        command: &command,
        console: None,
        inherit_environment: false,
        stdio: Some(Stdio {
            input: &child_input,
            output: &child_output,
            error: &stderr,
        }),
    })
    .map_err(|error| Error::guardian(&error))?;
    let input = File::from(input_write);
    input_without_waiting(&input)?;
    Ok(Spawned {
        child: Child::new(process, pid, job, None),
        input,
        output: Output::new(File::from(output_read))?,
        pid,
        resizer: None,
        guard: Some(Guard),
    })
}
