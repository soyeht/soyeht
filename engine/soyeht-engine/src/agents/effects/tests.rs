use super::*;
use crate::supervisor_link::tests::short_directory;
use std::fs;

fn write_request(path: &Path) -> NativeRequest {
    receive(
        "pane",
        1,
        1,
        json!(1),
        "fs/write_text_file".into(),
        json!({"sessionId":"s","path":path,"content":"#!/bin/sh\n"}),
        0,
    )
}

/// The write of an agent to a repository's own folder is refused in the effect itself, with a code
/// of its own and before anything is recorded (a file the editor tracks is written by a route that
/// does not go through the file API).
#[test]
fn an_agent_write_into_the_git_folder_is_refused_with_its_own_code() {
    let directory = short_directory();
    let mut core = Core::open(&directory).unwrap();
    fs::create_dir_all(directory.join(".git/hooks")).unwrap();
    let hook = directory.join(".git/hooks/pre-commit");
    let refused = core
        .native_file_write("pane", &directory, &hook, &write_request(&hook))
        .err()
        .unwrap();
    assert_eq!(refused.code, WRITE_REFUSED);
    assert!(!hook.exists(), "the hook was written");

    crate::test_paths::remove_after(core, directory);
}
