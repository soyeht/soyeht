//! `log::to_file` makes every log line also land in the file. One test: the file is
//! process-wide state, so it gets a process of its own. The bound is tested in `log::rotating`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::log;
use std::fs;

#[test]
fn lines_are_also_written_to_the_file() {
    let root = std::env::temp_dir().join(format!("soyeht-log-file-{}", std::process::id()));
    let path = root.join("logs").join("program.log");
    log::to_file(&path).unwrap();

    log::event("test.first", &[("key", "va\"lue")]);
    let error = std::io::Error::new(std::io::ErrorKind::NotFound, "/private/path");
    log::failure("test.failed", &error);
    log::Line::new("test.built").field("n", 3).emit();

    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        "{\"event\":\"test.first\",\"key\":\"va\\\"lue\"}\n\
         {\"event\":\"test.failed\",\"kind\":\"NotFound\"}\n\
         {\"event\":\"test.built\",\"n\":\"3\"}\n"
    );

    fs::remove_dir_all(root).ok();
}
