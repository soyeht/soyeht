//! The commands the seam asks the system to run, behind a trait so a test runs none.

use crate::error::{Error, Result};
use std::{process::Command, time::Duration};

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
        answer: Option<Answer>,
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
                answer: Some(Box::new(answer)),
            }
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
        fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput> {
            let mut call = vec![program.to_owned()];
            call.extend(arguments.iter().map(|argument| (*argument).to_owned()));
            self.calls.borrow_mut().push(call);
            let mut whole = vec![program];
            whole.extend_from_slice(arguments);
            Ok(self
                .answer
                .as_ref()
                .map_or_else(|| ok(""), |answer| answer(&whole)))
        }
    }
}
