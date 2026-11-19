//! The limit of open files: a process raises its own, and a program it starts gets the one the
//! process began with. One test, because the limit belongs to the whole process.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::{
    descriptors::{WANTED, raise, restore_in_child},
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
}
