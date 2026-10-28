//! The program a Windows scheduled task starts instead of the engine or the supervisor.
//!
//! A console program that Task Scheduler starts opens a console window and keeps no log. The
//! host (`soyeht-service-host`) is a Windows-subsystem program: it has no window of its own,
//! starts the real program with a hidden console and its output appended to a log file, waits for
//! it and exits with its exit code. The task therefore runs as long as the program does, a
//! failure reaches `RestartOnFailure`, and ending the task ends the program: the child stays in
//! the task's job.
//!
//! ```text
//! soyeht-service-host --log FILE -- PROGRAM [ARGUMENT]...
//! ```

use soyeht_platform::{log, process::detached::spawn_attached};
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// The file name of the host program, without a suffix.
pub(crate) const PROGRAM: &str = "soyeht-service-host";
/// The option that names the log file.
pub(crate) const LOG_OPTION: &str = "--log";
/// What ends the host's options and starts the program's command line.
pub(crate) const PROGRAM_SEPARATOR: &str = "--";

/// What the command line of the host says.
struct Invocation {
    log: PathBuf,
    program: PathBuf,
    arguments: Vec<OsString>,
}

fn parse(mut arguments: impl Iterator<Item = OsString>) -> Option<Invocation> {
    let mut log = None;
    while let Some(argument) = arguments.next() {
        if argument == LOG_OPTION {
            log = Some(PathBuf::from(arguments.next()?));
        } else if argument == PROGRAM_SEPARATOR {
            let program = PathBuf::from(arguments.next()?);
            return Some(Invocation {
                log: log?,
                program,
                arguments: arguments.collect(),
            });
        } else {
            return None;
        }
    }
    None
}

/// The host's own exit codes, apart from the program's: a command line it does not understand,
/// a program it could not start (the reason is in the log), and a log it could not write to, so
/// that Task Scheduler's "last run result" tells the three apart. The program's own code passes
/// through whole, so one of these can also be a program's.
const USAGE: i32 = 2;
const START_FAILED: i32 = 1;
const LOG_UNWRITABLE: i32 = 3;

/// Runs the program the command line names and returns its exit code, whole: a Windows exit
/// code is 32 bits wide. A program ended without a code (by a signal) fails the host, so the
/// task restarts it.
pub fn run(arguments: impl Iterator<Item = OsString>) -> i32 {
    let Some(invocation) = parse(arguments) else {
        return USAGE;
    };
    match start(&invocation) {
        Ok(code) => code,
        Err(error) => {
            // The host has no console, so the log file is the only place to say why. When even
            // that cannot be opened there is nowhere left to write, and the exit code is all
            // that Task Scheduler's history keeps.
            if log::to_file(&invocation.log).is_err() {
                return LOG_UNWRITABLE;
            }
            log::failure("service_host.start_failed", &error);
            START_FAILED
        }
    }
}

fn start(invocation: &Invocation) -> std::io::Result<i32> {
    if let Some(folder) = invocation.log.parent() {
        // Owner-only like the logs folder `install` makes, whoever made it first.
        crate::manage::private_folder(folder).map_err(std::io::Error::other)?;
    }
    let arguments: Vec<&OsStr> = invocation
        .arguments
        .iter()
        .map(OsString::as_os_str)
        .collect();
    let mut child = spawn_attached(Path::new(&invocation.program), &arguments, &invocation.log)?;
    Ok(child.wait()?.code().unwrap_or(START_FAILED))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(words: &[&str]) -> impl Iterator<Item = OsString> {
        words
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn the_log_and_the_program_with_its_arguments_are_read() {
        let invocation = parse(words(&[
            "--log",
            "/l/engine.log",
            "--",
            "/p/engine",
            "--data-dir",
            "/d",
        ]))
        .unwrap();
        assert_eq!(invocation.log, PathBuf::from("/l/engine.log"));
        assert_eq!(invocation.program, PathBuf::from("/p/engine"));
        assert_eq!(invocation.arguments, ["--data-dir", "/d"]);
        // An option of the program is the program's, not the host's.
        let invocation = parse(words(&["--log", "/l", "--", "/p", "--log", "x"])).unwrap();
        assert_eq!(invocation.arguments, ["--log", "x"]);
    }

    #[test]
    fn a_command_line_without_a_log_a_program_or_with_something_else_is_refused() {
        for line in [
            &[][..],
            &["--", "/p"],
            &["--log"],
            &["--log", "/l"],
            &["--log", "/l", "--"],
            &["--other"],
        ] {
            assert!(parse(words(line)).is_none(), "{line:?}");
        }
    }
}
