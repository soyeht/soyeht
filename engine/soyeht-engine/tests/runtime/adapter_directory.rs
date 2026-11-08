//! A built-in adapter starts in the engine's own folder, and works in the session's.

use super::*;

/// The programs that write the folder they were started in to `seen`, then start the fake agent:
/// the package runner of a custom entry, and the `node` that runs a locked adapter's installed
/// script (the first start installs it with the rig's `npm`).
#[cfg(unix)]
fn install_recording_runner(rig: &Rig, seen: &Path) {
    rig.install_fake_locked_adapter_runner(&format!("pwd -P > '{}'", seen.display()));
    write_script(
        &rig.path.join("bin/npx"),
        &format!(
            "#!/bin/sh\npwd -P > '{}'\nexec '{}' --state-dir '{}'\n",
            seen.display(),
            sibling_binary("soyeht-fake-acp").display(),
            rig.path.join("claude-native").display()
        ),
    );
}

#[cfg(unix)]
#[test]
fn a_package_runner_adapter_starts_in_the_engines_folder_and_works_in_the_workspace() {
    let rig = Rig::new();
    let seen = rig.path.join("adapter-started-in");
    install_recording_runner(&rig, &seen);
    let repository = rig.path.join("repository");
    fs::create_dir(&repository).unwrap();
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Repository", &repository, "adapter-workspace");
    let pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"registry.claude-acp",
            "idempotencyKey":"adapter-pane"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_until("the adapter did not start", || {
        client.pane(&pane)["agent"]["sessionId"].is_string() && seen.exists()
    });

    // The process starts in the engine's folder, not in the workspace.
    let started_in = fs::read_to_string(&seen).unwrap();
    assert_eq!(
        Path::new(started_in.trim_end()),
        soyeht_platform::fs::canonicalize(&rig.path.join("adapters")).unwrap()
    );

    // The session is opened on the workspace: the adapter's file operations land there.
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"tools","idempotencyKey":"adapter-tools"}),
    );
    wait_until("the adapter did not write in the session's folder", || {
        repository.join("fake-acp-note.txt").exists()
    });
    assert!(!rig.path.join("adapters/fake-acp-note.txt").exists());

    // A folder that was removed is made again by the next start.
    fs::remove_dir_all(rig.path.join("adapters")).unwrap();
    fs::remove_file(&seen).unwrap();
    let second = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"registry.claude-acp",
            "idempotencyKey":"adapter-pane-again"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_until("the adapter did not start in the folder made again", || {
        client.pane(&second)["agent"]["sessionId"].is_string() && seen.exists()
    });
    assert!(rig.path.join("adapters").is_dir());
}

#[cfg(unix)]
#[test]
fn a_custom_harness_that_runs_npx_keeps_the_panes_folder() {
    let rig = Rig::new();
    let seen = rig.path.join("custom-started-in");
    install_recording_runner(&rig, &seen);
    rig.write_harnesses(&json!([{
        "id":"local-npx-agent","name":"Local agent","command":"npx",
        "args":["./local-agent"],"version":"test"
    }]));
    let repository = rig.path.join("repository");
    fs::create_dir(&repository).unwrap();
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Repository", &repository, "custom-workspace");
    let pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"local-npx-agent",
            "idempotencyKey":"custom-pane"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_until("the custom adapter did not start", || {
        client.pane(&pane)["agent"]["sessionId"].is_string() && seen.exists()
    });

    // It runs as its author wrote it: relative paths in its arguments are the workspace's.
    let started_in = fs::read_to_string(&seen).unwrap();
    assert_eq!(
        Path::new(started_in.trim_end()),
        soyeht_platform::fs::canonicalize(&repository).unwrap()
    );
}
