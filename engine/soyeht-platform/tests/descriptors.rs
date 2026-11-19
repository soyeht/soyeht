//! The limit of open files: a process raises its own, and a program it starts gets the one the
//! process began with. One test, because the limit belongs to the whole process.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::{
    descriptors::{WANTED, raise, restore_in_child},
    process::detached::spawn_detached,
    testing::{lower_open_files_limit, open_files_limit},
};
use std::process::Command;

fn limit_of(command: &mut Command) -> u64 {
    let output = command.args(["-c", "ulimit -n"]).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[test]
fn a_started_program_does_not_inherit_the_raised_limit() {
    // The limit of a service that the system started.
    lower_open_files_limit(256).unwrap();
    let raised = raise().unwrap();
    assert_eq!(raised.before.soft, Some(256));
    let now = raised.now.soft.unwrap();
    assert!(now > 256 && now <= WANTED, "{now}");
    assert_eq!(open_files_limit().unwrap(), now);

    assert_eq!(limit_of(&mut Command::new("sh")), now);
    let mut restored = Command::new("sh");
    restore_in_child(&mut restored);
    assert_eq!(limit_of(&mut restored), 256);

    // A second raise has nothing to do, and the children still get what the process began with.
    let again = raise().unwrap();
    assert_eq!(again.before, again.now);
    let mut restored = Command::new("sh");
    restore_in_child(&mut restored);
    assert_eq!(limit_of(&mut restored), 256);

    // A descriptor numbered above the limit the process started with, open after the raise, does
    // not reach a program started detached: the walk that closes them reads the raised limit.
    let mut ends = [0; 2];
    // SAFETY: `ends` holds the two descriptors the call fills.
    assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
    // SAFETY: duplicates a descriptor this test opened to the first free number from 300, which
    // the standard library marks close-on-exec in no way: the copy is inherited.
    let high = unsafe { libc::fcntl(ends[0], libc::F_DUPFD, 300) };
    assert!(high >= 300, "{high}");
    let script = format!("test -e /dev/fd/{high} && echo open || echo closed");
    let log = soyeht_platform::testing::scratch_dir("descriptors-high").join("log");
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    let plain = Command::new("sh").args(["-c", &script]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&plain.stdout).trim(), "open");
    let script = std::ffi::OsString::from(script);
    let arguments = [std::ffi::OsStr::new("-c"), script.as_os_str()];
    spawn_detached(std::path::Path::new("sh"), &arguments, &log)
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "closed");
    std::fs::remove_dir_all(log.parent().unwrap()).unwrap();
    // SAFETY: closes the three descriptors this test opened.
    unsafe {
        libc::close(high);
        libc::close(ends[0]);
        libc::close(ends[1]);
    }
}
