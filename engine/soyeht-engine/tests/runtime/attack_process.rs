//! Injection into the commands the engine starts. The text a person or an agent types (a task's
//! title and description) and the folder a workspace lives in reach `wt` and `git` only as whole
//! arguments: nothing runs through a shell, and nothing the text says becomes an option.
#![cfg(unix)]
use super::*;

const HOSTILE: [&str; 6] = [
    "$(touch PWNED)",
    "`touch PWNED`",
    "; touch PWNED #",
    "--force --no-hooks",
    "x\n--create evil",
    "'; touch PWNED; '",
];

#[test]
fn hostile_text_and_folder_names_reach_the_worktree_command_only_as_whole_arguments() {
    let rig = Rig::new();
    rig.install_fake_harness("config");
    let folder = rig.path.join("--help $(touch PWNED) `touch PWNED`; x");
    fs::create_dir_all(&folder).unwrap();
    let folder = soyeht_platform::fs::canonicalize(&folder).unwrap();
    let log = rig.path.join("wt-arguments.log");
    let worktrunk = rig.path.join("bin/wt");
    let real = fs::read_link(&worktrunk).unwrap();
    replace_link_with_script(
        &worktrunk,
        &format!(
            "#!/bin/sh\nprintf 'CALL\\n' >> '{log}'\nfor argument in \"$@\"; do printf '%s\\n' \"$argument\" >> '{log}'; done\nexec '{real}' \"$@\"\n",
            log = log.display(),
            real = real.display(),
        ),
    );
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "--version", &folder, "hostile-workspace");
    for (index, text) in HOSTILE.iter().enumerate() {
        let created = client.request(
            "task.create",
            json!({"workspaceId":workspace,"title":text,"description":text,"howToVerify":text,
                "mustNotChange":text,"createWorktree":true,"dispatch":true,
                "executor":{"workspaceDefault":false,"harness":"test-agent","permission":"ask"},
                "idempotencyKey":format!("hostile-{index}")}),
        );
        assert!(created.get("error").is_none(), "{text:?}: {created}");
    }
    wait_until("a worktree for every task", || {
        fs::read_to_string(&log)
            .is_ok_and(|calls| calls.matches("\nswitch\n").count() >= HOSTILE.len())
    });
    let calls = fs::read_to_string(&log).unwrap_or_default();
    let lines: Vec<&str> = calls.lines().collect();
    assert!(!lines.is_empty(), "the worktree command was never started");
    // Every call is `-C <the folder, whole> <flags> ... task/<id>`; nothing the text said is in it.
    for line in &lines {
        assert!(
            !HOSTILE
                .iter()
                .any(|text| text.lines().any(|part| part == *line)),
            "hostile text became an argument: {line:?}"
        );
    }
    let folder_text = folder.display().to_string();
    for call in calls.split("CALL\n").filter(|call| !call.is_empty()) {
        let arguments: Vec<&str> = call.lines().collect();
        assert_eq!(arguments[0], "-C", "{arguments:?}");
        assert_eq!(arguments[1], folder_text, "{arguments:?}");
        let branch = arguments.last().unwrap();
        assert!(
            branch.starts_with("task/ws-") || arguments.contains(&"list"),
            "{arguments:?}"
        );
    }
    // Nothing was executed: the canary a command substitution would touch does not exist.
    for place in [&rig.path, &folder, &std::env::current_dir().unwrap()] {
        assert!(!place.join("PWNED").exists(), "{place:?}");
    }
}
