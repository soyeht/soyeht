//! The commands the seam asks the system to run, behind a trait so a test runs none.

use crate::error::{Error, Result};
use std::{io, process::Command, time::Duration};

/// How long a service-manager command may run. `launchctl bootstrap` and `systemctl enable
/// --now` wait for a program to start, which on a loaded host takes a while.
const TIMEOUT: Duration = Duration::from_secs(60);
const OUTPUT_BYTES: usize = 64 * 1024;

/// What a command printed and whether it succeeded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs the service manager's own tool (`launchctl`, `systemctl`, `schtasks`, `powershell`).
pub trait Commands {
    /// Runs `program` with `arguments` and waits for it, within a deadline.
    fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput>;

    /// Asks the program that runs as process `pid` to end (a stop request, or at once on
    /// Windows). A test records the id and ends nothing.
    fn terminate(&self, pid: u32) -> io::Result<()> {
        soyeht_platform::process::terminate(pid)
    }

    /// Waits before a command is tried again. A test waits for nothing.
    fn pause(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Runs the commands for real, through the engine's bounded runner (ADR 0079).
pub struct Real;

impl Commands for Real {
    fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput> {
        let mut command = Command::new(program);
        command.args(arguments);
        let output =
            soyeht_process::run(command, soyeht_process::Limits::new(TIMEOUT, OUTPUT_BYTES))
                .map_err(Error::from)?;
        Ok(CommandOutput {
            success: output.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: output.stderr_text(),
        })
    }
}

/// Runs a command that must succeed; `step` names it in the error.
pub(crate) fn require(
    commands: &impl Commands,
    step: &'static str,
    program: &str,
    arguments: &[&str],
) -> Result<CommandOutput> {
    let output = commands.run(program, arguments)?;
    if output.success {
        Ok(output)
    } else {
        Err(Error::Failed {
            step,
            detail: output.stderr,
        })
    }
}

/// A recording stand-in for the system, which answers from a script.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::cell::RefCell;

    type Answer = Box<dyn Fn(&[&str]) -> CommandOutput>;

    /// Records every command and answers each by looking at its program and arguments.
    #[derive(Default)]
    pub(crate) struct Recorder {
        pub(crate) calls: RefCell<Vec<Vec<String>>>,
        /// The waits asked for, in order; none of them took any time.
        pub(crate) pauses: RefCell<Vec<Duration>>,
        answer: Option<Answer>,
        /// Commands (as one line) that print more than the budget allows.
        too_large: Vec<String>,
        /// The process ids asked to end, in order.
        pub(crate) terminated: RefCell<Vec<u32>>,
        /// What a program that is asked to end does: a test drops the lock it holds.
        on_terminate: Option<Box<dyn Fn(u32)>>,
        /// The first request to end a program is refused with this error.
        refuse_first: std::cell::Cell<Option<io::ErrorKind>>,
    }

    pub(crate) fn ok(stdout: &str) -> CommandOutput {
        CommandOutput {
            success: true,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    pub(crate) fn no() -> CommandOutput {
        CommandOutput::default()
    }

    impl Recorder {
        pub(crate) fn answering(answer: impl Fn(&[&str]) -> CommandOutput + 'static) -> Self {
            Self {
                calls: RefCell::default(),
                pauses: RefCell::default(),
                answer: Some(Box::new(answer)),
                too_large: Vec::new(),
                terminated: RefCell::default(),
                on_terminate: None,
                refuse_first: std::cell::Cell::default(),
            }
        }

        /// The system refuses the first request to end a program, as it does for a process of
        /// another account.
        pub(crate) fn refusing_the_first_end(self, kind: io::ErrorKind) -> Self {
            self.refuse_first.set(Some(kind));
            self
        }

        /// A program that is asked to end runs `ending`, with its process id, as it would stop.
        pub(crate) fn ending_programs_with(mut self, ending: impl Fn(u32) + 'static) -> Self {
            self.on_terminate = Some(Box::new(ending));
            self
        }

        /// The command that reads exactly `line` is ended by the runner for printing too much.
        pub(crate) fn printing_too_much(mut self, line: &str) -> Self {
            self.too_large.push(line.to_owned());
            self
        }

        /// Every command as one line, `program argument ...`.
        pub(crate) fn lines(&self) -> Vec<String> {
            self.calls
                .borrow()
                .iter()
                .map(|call| call.join(" "))
                .collect()
        }
    }

    impl Commands for Recorder {
        fn terminate(&self, pid: u32) -> io::Result<()> {
            self.terminated.borrow_mut().push(pid);
            if let Some(kind) = self.refuse_first.take() {
                return Err(kind.into());
            }
            if let Some(ending) = &self.on_terminate {
                ending(pid);
            }
            Ok(())
        }

        fn pause(&self, duration: Duration) {
            self.pauses.borrow_mut().push(duration);
        }

        fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput> {
            let mut call = vec![program.to_owned()];
            call.extend(arguments.iter().map(|argument| (*argument).to_owned()));
            self.calls.borrow_mut().push(call);
            let mut whole = vec![program];
            whole.extend_from_slice(arguments);
            if self.too_large.contains(&whole.join(" ")) {
                return Err(Error::Process(soyeht_process::Error::OutputTooLarge {
                    program: program.to_owned(),
                }));
            }
            Ok(self
                .answer
                .as_ref()
                .map_or_else(|| ok(""), |answer| answer(&whole)))
        }
    }
}
