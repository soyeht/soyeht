//! The Claude adapter drives the person's own Claude Code when it is new enough (ADR 0137).

use super::*;

/// The `node` that runs the installed Claude adapter (the first start installs it with the rig's
/// `npm`). It writes the Claude Code executable its adapter was told to drive (nothing when it
/// was told none) to the returned file, then starts the fake agent.
#[cfg(unix)]
fn install_recording_runner(rig: &Rig) -> PathBuf {
    let seen = rig.path.join("claude-executable-seen");
    rig.install_fake_locked_adapter_runner(&format!(
        "printf '%s' \"$CLAUDE_CODE_EXECUTABLE\" > '{}'",
        seen.display()
    ));
    seen
}

/// A `claude` on the engine's search path that answers `--version` with `version`, as an
/// installer leaves it: a link to the file of that version, which an update points elsewhere.
#[cfg(unix)]
fn install_claude(rig: &Rig, version: &str) {
    let versions = rig.path.join("claude-versions");
    fs::create_dir_all(&versions).unwrap();
    let file = versions.join(version);
    write_script(
        &file,
        &format!("#!/bin/sh\necho '{version} (Claude Code)'\n"),
    );
    let link = rig.path.join("bin/claude");
    if link.symlink_metadata().is_ok() {
        fs::remove_file(&link).unwrap();
    }
    std::os::unix::fs::symlink(file, link).unwrap();
}

/// Opens an agent pane on the Claude harness and returns the Claude Code executable its adapter
/// was told to drive: empty when it was told none.
#[cfg(unix)]
fn executable_the_adapter_is_told(
    client: &mut Client,
    workspace: &str,
    seen: &Path,
    key: &str,
) -> String {
    if seen.exists() {
        fs::remove_file(seen).unwrap();
    }
    let pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"registry.claude-acp",
            "idempotencyKey":key}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while !(client.pane(&pane)["agent"]["sessionId"].is_string() && seen.exists()) {
        assert!(
            Instant::now() < deadline,
            "the adapter did not start: {}\n{}",
            client.pane(&pane),
            fs::read_to_string(seen.with_file_name("engine.log")).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(20));
    }
    let told = fs::read_to_string(seen).unwrap();
    client.call(
        "pane.close",
        json!({"paneId":pane,"idempotencyKey":format!("{key}-close")}),
    );
    told
}

#[cfg(unix)]
#[test]
fn the_adapter_drives_the_claude_on_the_search_path_only_while_it_is_new_enough() {
    let rig = Rig::with_own_search_path();
    let seen = install_recording_runner(&rig);
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Own CLI", &rig.path, "own-cli-workspace");
    let told = |client: &mut Client, key: &str| {
        executable_the_adapter_is_told(client, &workspace, &seen, key)
    };

    // None installed: the adapter keeps the one it bundles.
    assert_eq!(told(&mut client, "none"), "");

    // Installed and new enough: the adapter drives it, by the path the search path has, which an
    // update keeps right, and not by the file of one version.
    install_claude(&rig, "2.1.312");
    let own = rig.path.join("bin/claude");
    assert_eq!(told(&mut client, "new-enough"), own.to_str().unwrap());

    // Updated in place to a version the current models refuse: the bundled one again.
    install_claude(&rig, "2.1.270");
    assert_eq!(told(&mut client, "too-old"), "");

    // Updated again, the link now pointing at another file: it is asked once more.
    install_claude(&rig, "2.1.1000");
    assert_eq!(told(&mut client, "updated"), own.to_str().unwrap());

    // The log says what was chosen and why, and never a path.
    let log = fs::read_to_string(rig.path.join("engine.log")).unwrap();
    let chosen: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("engine.agent_cli_chosen"))
        .collect();
    assert_eq!(chosen.len(), 4, "{chosen:?}");
    assert!(
        chosen[0].contains("\"why\":\"not_installed\""),
        "{chosen:?}"
    );
    assert!(chosen[1].contains("\"choice\":\"own\""), "{chosen:?}");
    assert!(chosen[2].contains("\"why\":\"too_old\""), "{chosen:?}");
    assert!(chosen[3].contains("\"choice\":\"own\""), "{chosen:?}");
    assert!(chosen.iter().all(|line| !line.contains("bin/claude")));
}

#[cfg(unix)]
#[test]
fn a_switch_to_the_harness_gives_its_adapter_the_claude_the_person_has() {
    let rig = Rig::with_own_search_path();
    let seen = install_recording_runner(&rig);
    install_claude(&rig, "2.1.312");
    let (mut client, pane) = rig.agent();
    assert!(
        !seen.exists(),
        "the first harness is not the one under test"
    );

    let result = client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"registry.claude-acp","idempotencyKey":"switch-to-claude"}),
    );
    assert_eq!(result["toHarness"], "registry.claude-acp");
    wait_until("the switched adapter did not start", || seen.exists());
    assert_eq!(
        fs::read_to_string(&seen).unwrap(),
        rig.path.join("bin/claude").to_str().unwrap()
    );
}
