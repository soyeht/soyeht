//! The foreground program of a terminal on macOS and Linux.

use super::{Program, bounded};
use std::{io, os::fd::AsRawFd};

/// The program that leads the foreground process group of the terminal whose master side is
/// `terminal`: empty when the group has no leader left to read (it ended while its children
/// went on) or the process cannot be read. A terminal with no foreground group is an error.
///
/// # Errors
/// The system's error when `terminal` is not a terminal master.
pub fn foreground_of_terminal(terminal: &impl AsRawFd) -> io::Result<Vec<Program>> {
    // SAFETY: the descriptor is open for the call, which only reads the terminal's state.
    let group = unsafe { libc::tcgetpgrp(terminal.as_raw_fd()) };
    if group == -1 {
        return Err(io::Error::last_os_error());
    }
    let Ok(leader) = u32::try_from(group) else {
        return Ok(Vec::new());
    };
    Ok(program_of(leader).into_iter().collect())
}

/// What the process `pid` runs, or `None` when it has ended or cannot be read (another user's).
fn program_of(pid: u32) -> Option<Program> {
    let (executable, arguments) = read_process(pid)?;
    Some(Program {
        pid,
        parent: None,
        executable,
        arguments: bounded(arguments),
    })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn read_process(pid: u32) -> Option<(Option<String>, Vec<String>)> {
    // ignored: the process may have ended since it was named; it then has no program to report
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let executable = std::fs::read_link(format!("/proc/{pid}/exe"))
        // ignored: a process of another user, or one that ended, has no readable file; its
        // arguments still name it
        .ok()
        // ignored: a path that is not text is not a harness's
        .and_then(|path| path.into_os_string().into_string().ok())
        // A replaced file reads as "<path> (deleted)"; the harness is still the one at the path.
        .map(|path| match path.strip_suffix(" (deleted)") {
            Some(live) => live.to_owned(),
            None => path,
        });
    Some((executable, split_nul(&cmdline)))
}

/// The NUL-separated words of a process's command line, as text.
#[cfg(any(target_os = "linux", target_os = "android", test))]
fn split_nul(bytes: &[u8]) -> Vec<String> {
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    if bytes.is_empty() {
        return Vec::new();
    }
    bytes
        .split(|byte| *byte == 0)
        .map(|word| String::from_utf8_lossy(word).into_owned())
        .collect()
}

#[cfg(target_os = "macos")]
fn read_process(pid: u32) -> Option<(Option<String>, Vec<String>)> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is as long as the size says, and the call fills at most that.
    let written = unsafe {
        libc::proc_pidpath(
            pid,
            path.as_mut_ptr().cast(),
            u32::try_from(path.len()).ok()?,
        )
    };
    let executable = usize::try_from(written)
        .ok()
        .filter(|written| *written > 0)
        .and_then(|written| String::from_utf8(path.get(..written)?.to_vec()).ok());
    let (_, arguments) = parse_process_arguments(&process_arguments(pid)?)?;
    Some((executable, arguments))
}

/// What `sysctl(KERN_PROCARGS2)` holds for `pid`: the argument count, the executable path, then
/// the arguments and the environment.
#[cfg(target_os = "macos")]
fn process_arguments(pid: libc::pid_t) -> Option<Vec<u8>> {
    let mut name = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: libc::size_t = 0;
    // SAFETY: a null buffer only asks how many bytes the answer takes; `name` has 3 entries.
    let asked = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if asked == -1 || size == 0 {
        return None;
    }
    // The process may have started more since the size was asked.
    let mut buffer = vec![0u8; size + size / 4];
    let mut size = buffer.len();
    // SAFETY: the buffer is `size` bytes, which the call updates to the bytes it filled.
    let read = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read == -1 {
        return None;
    }
    buffer.truncate(size);
    Some(buffer)
}

/// The executable path and `argv` of a `KERN_PROCARGS2` answer: a native-endian `argc`, the path
/// and its NUL padding, then `argc` NUL-terminated arguments (the environment follows and is
/// never read).
#[cfg(any(target_os = "macos", test))]
fn parse_process_arguments(buffer: &[u8]) -> Option<(String, Vec<String>)> {
    let count = usize::try_from(i32::from_ne_bytes(buffer.get(..4)?.try_into().ok()?)).ok()?;
    let rest = buffer.get(4..)?;
    let path_end = rest.iter().position(|byte| *byte == 0)?;
    let path = String::from_utf8_lossy(&rest[..path_end]).into_owned();
    let mut rest = &rest[path_end..];
    let padding = rest.iter().take_while(|byte| **byte == 0).count();
    rest = &rest[padding..];
    let mut arguments = Vec::new();
    for _ in 0..count.min(super::MAX_ARGUMENTS) {
        let end = rest
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(rest.len());
        arguments.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        rest = rest.get(end + 1..).unwrap_or(&[]);
    }
    Some((path, arguments))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::serialize_processes;
    use std::{
        fs::File,
        os::fd::FromRawFd,
        process::{Command, Stdio},
    };

    #[test]
    fn a_command_line_is_split_at_its_nul_bytes() {
        assert_eq!(split_nul(b"claude\0--flag\0"), ["claude", "--flag"]);
        assert_eq!(split_nul(b"a\0\0b\0"), ["a", "", "b"]);
        assert!(split_nul(b"").is_empty());
    }

    #[test]
    fn a_macos_argument_block_gives_the_path_and_the_arguments_only() {
        let mut block = 3i32.to_ne_bytes().to_vec();
        block.extend(b"/home/x/.local/share/cursor-agent/versions/2026.09.02/node\0\0\0");
        block.extend(b"/home/x/.local/bin/cursor-agent\0--use-system-ca\0index.js\0");
        block.extend(b"HOME=/home/x\0PATH=/bin\0");
        let (path, arguments) = parse_process_arguments(&block).unwrap();
        assert_eq!(
            path,
            "/home/x/.local/share/cursor-agent/versions/2026.09.02/node"
        );
        assert_eq!(
            arguments,
            [
                "/home/x/.local/bin/cursor-agent",
                "--use-system-ca",
                "index.js"
            ]
        );
        assert!(parse_process_arguments(b"\x01").is_none());
    }

    #[test]
    fn the_foreground_of_a_terminal_is_the_program_that_leads_its_group() {
        let _serial = serialize_processes();
        let _forks = crate::spawning();
        let mut master = -1;
        let mut slave = -1;
        // SAFETY: openpty gets live out pointers and no name, termios or size.
        let opened = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(opened, 0);
        // SAFETY: openpty returned two descriptors that nothing else owns.
        let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        let mut child = {
            use std::os::unix::process::CommandExt;
            let mut command = Command::new("sleep");
            command
                .arg("30")
                .stdin(Stdio::from(slave.try_clone().unwrap()))
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // SAFETY: only async-signal-safe calls between fork and exec.
            unsafe {
                command.pre_exec(|| {
                    let session = libc::setsid();
                    let terminal = libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                    if session == -1 || terminal == -1 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
            command.spawn().unwrap()
        };
        let found = foreground_of_terminal(&master).unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pid, child.id());
        assert_eq!(found[0].arguments, ["sleep", "30"]);
        let executable = found[0].executable.as_deref().unwrap();
        assert!(executable.ends_with("sleep"), "{executable}");
    }

    #[test]
    fn a_file_that_is_not_a_terminal_has_no_foreground() {
        let file = File::open("/dev/null").unwrap();
        assert!(foreground_of_terminal(&file).is_err());
    }
}
