use super::{Resizer, Session};
use crate::{
    child::Child,
    io::{Output, input_without_waiting},
};
use soyeht_platform::process::windows::{self as winproc, Console, Job, Launch};
use std::{fs::File, io, process::Command, sync::Arc};

/// The pseudo console of the session.
pub(super) type Terminal = Console;

pub(super) fn spawn(command: Command, rows: u16, cols: u16) -> io::Result<Session> {
    let (input_read, input_write) = winproc::pipe()?;
    let (output_read, output_write) = winproc::pipe()?;
    let console = Arc::new(
        Console::new(rows, cols, &input_read, &output_write).map_err(crate::error::setup_failed)?,
    );
    // The console now holds its own ends of both pipes.
    drop((input_read, output_write));
    let job = Job::new().map_err(crate::error::setup_failed)?;
    let (process, pid) = winproc::create_process(&Launch {
        command: &command,
        job: &job,
        console: Some(&console),
        stdio: None,
        inherit_environment: false,
    })?;
    let input = File::from(input_write);
    input_without_waiting(&input)?;
    Ok(Session {
        child: Child::new(process, pid, job, Some(Arc::clone(&console))),
        input,
        output: Output::new(File::from(output_read))?,
        resizer: Resizer(console),
    })
}
