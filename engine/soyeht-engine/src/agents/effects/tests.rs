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

/// The write of an agent to a repository's own folder is refused in the effect itself, with the
/// code of a refused argument and before anything is recorded (a file the editor tracks is written by a route that
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
    assert_eq!(
        i64::from(refused.code),
        error_code(&ErrorKind::InvalidParams),
        "an adapter reads -32002 as a missing session"
    );
    assert!(!hook.exists(), "the hook was written");

    crate::test_paths::remove_after(core, directory);
}

/// The write itself refuses the folder too; the effect answers that with the same code, and a
/// failure of another kind stays a failure.
#[test]
fn the_refusal_of_the_write_itself_keeps_the_code_of_the_effects_check() {
    let directory = short_directory();
    let hook = directory.join(".git/hooks/pre-commit");
    let error = filesystem::write(&directory, &hook, "#!/bin/sh\n").unwrap_err();
    let answer = write_failure(&error);
    assert_eq!(answer.code, write_refused("x").code);
    assert_ne!(answer.code, write_failure(&errors::invalid("other")).code);
    assert_eq!(
        write_failure(&errors::invalid("other")).code,
        failure("x").code
    );
    assert!(!directory.join(".git").exists());
    let _ = fs::remove_dir_all(&directory);
}
