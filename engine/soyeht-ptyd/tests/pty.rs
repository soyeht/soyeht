//! The pseudo-terminal session, without the supervisor around it; the same on every system.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_ptyd::{child::Reach, io::Output, pty};
use std::{
    io::{Read, Write},
    process::Command,
    time::{Duration, Instant},
};

fn shell(script: &str) -> Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd.exe");
        command.args(["/c", script]);
        command
    } else {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }
}

/// Reads until `needle` shows up, returning everything read.
fn read_until(output: &mut Output, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut text = Vec::new();
    let mut chunk = [0_u8; 0x1000];
    loop {
        match output.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => text.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                output.wait_readable(Duration::from_millis(100)).unwrap();
            }
            Err(error) => panic!("{error}"),
        }
        if String::from_utf8_lossy(&text).contains(needle) || Instant::now() > deadline {
            break;
        }
    }
    let text = String::from_utf8_lossy(&text).into_owned();
    assert!(text.contains(needle), "missing {needle:?} in {text:?}");
    text
}

#[test]
fn what_a_child_prints_before_it_exits_is_still_readable() {
    let mut session = pty::spawn(shell("echo printed-then-gone"), 24, 80).unwrap();
    read_until(&mut session.output, "printed-then-gone");
    assert_eq!(session.child.wait().unwrap().code(), Some(0));
}

#[test]
fn the_exit_code_of_the_child_is_reported() {
    let mut session = pty::spawn(shell("exit 7"), 24, 80).unwrap();
    assert_eq!(session.child.wait().unwrap().code(), Some(7));
}

#[test]
fn typed_input_reaches_the_child_and_a_resize_is_accepted() {
    let program = if cfg!(windows) { "cmd.exe" } else { "/bin/cat" };
    let mut session = pty::spawn(Command::new(program), 24, 80).unwrap();
    session.resizer.resize(100, 30).unwrap();
    assert!(session.resizer.resize(0, 30).is_err());
    session.input.write_all(b"typed-line\r\n").unwrap();
    read_until(&mut session.output, "typed-line");
    session.child.kill(Reach::Group);
    session.child.wait().unwrap();
}

#[test]
fn a_killed_terminal_is_collected_within_a_bound_even_when_nobody_reads_its_output() {
    let program = if cfg!(windows) { "cmd.exe" } else { "/bin/cat" };
    let mut session = pty::spawn(Command::new(program), 24, 80).unwrap();
    session.input.write_all(b"unread-output\r\n").unwrap();
    session.child.kill(Reach::Group);
    let exit = session
        .child
        .wait_within(std::time::Duration::from_secs(10))
        .unwrap();
    assert!(exit.is_some());
}

/// Runs a program that interrupts itself in a terminal and checks that the signal ended it.
#[cfg(unix)]
fn assert_interrupt_ends_a_program() {
    let mut session = pty::spawn(shell("kill -INT $$; echo survived"), 24, 80).unwrap();
    let exit = session
        .child
        .wait_within(Duration::from_secs(10))
        .unwrap()
        .expect("the program was not interrupted");
    assert_eq!(exit.code(), None, "the program ended on its own");
}

/// Makes this process ignore Ctrl+C for as long as it lives, as a supervisor started in the
/// background of a non-interactive shell does; restores what was there.
#[cfg(unix)]
struct IgnoringInterrupts(libc::sighandler_t);

#[cfg(unix)]
impl IgnoringInterrupts {
    fn new() -> Self {
        // SAFETY: SIGINT is a valid signal and SIG_IGN a valid disposition.
        Self(unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) })
    }
}

#[cfg(unix)]
impl Drop for IgnoringInterrupts {
    fn drop(&mut self) {
        // SAFETY: restores the disposition the constructor read.
        unsafe { libc::signal(libc::SIGINT, self.0) };
    }
}

#[cfg(unix)]
#[test]
fn an_interrupt_ends_a_terminal_program_even_when_the_supervisor_ignores_it() {
    let _ignoring = IgnoringInterrupts::new();
    assert_interrupt_ends_a_program();
}

/// Blocks Ctrl+C on the calling thread, as a supervisor started with it blocked has it; restores
/// the mask of the thread.
#[cfg(unix)]
struct BlockingInterrupts(libc::sigset_t);

#[cfg(unix)]
impl BlockingInterrupts {
    fn new() -> Self {
        // SAFETY: the sets are initialised by sigemptyset and sigaddset before they are used, and
        // pthread_sigmask changes only the mask of this thread.
        unsafe {
            let mut blocked: libc::sigset_t = std::mem::zeroed();
            let mut before: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGINT);
            libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut before);
            Self(before)
        }
    }
}

#[cfg(unix)]
impl Drop for BlockingInterrupts {
    fn drop(&mut self) {
        // SAFETY: restores the mask the constructor read.
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut()) };
    }
}

#[cfg(unix)]
#[test]
fn an_interrupt_ends_a_terminal_program_even_when_the_supervisor_blocks_it() {
    let _blocking = BlockingInterrupts::new();
    assert_interrupt_ends_a_program();
}
