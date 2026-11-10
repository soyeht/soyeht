use super::*;
use soyeht_protocol::ErrorKind;
#[test]
fn catalogue_pages_preserve_large_entries_and_reject_changed_cursors() {
    let items = (0..5)
        .map(|n| json!({"id":n,"text":"x".repeat(400_000)}))
        .collect::<Vec<_>>();
    let first = page(items.clone(), json!({}), &json!({})).unwrap();
    assert!(serde_json::to_vec(&first).unwrap().len() <= limits::JSON_BYTES);
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    let second = page(
        items.clone(),
        json!({}),
        &json!({"cursor":first["nextCursor"]}),
    )
    .unwrap();
    assert_eq!(second["items"][0]["id"], 2);
    let mut changed = items;
    changed[0]["text"] = json!("changed");
    assert_eq!(
        errors::kind(
            &page(changed, json!({}), &json!({"cursor":first["nextCursor"]})).unwrap_err()
        ),
        ErrorKind::ResyncRequired
    );
}

#[cfg(unix)]
#[test]
fn model_list_reads_an_executable_only_for_a_harness_with_a_live_session() {
    use std::os::unix::fs::PermissionsExt;
    let directory =
        std::env::temp_dir().join(format!("soyeht-model-list-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    // Executable but unreadable: resolving it works, hashing it cannot.
    let executable = directory.join("agent");
    std::fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o111)).unwrap();
    let launch = soyeht_protocol::HarnessLaunch {
        id: "slow-disk".into(),
        name: "Slow disk".into(),
        command: executable.to_str().unwrap().into(),
        args: vec![],
        version: "1".into(),
        environment: None,
    };
    registry::create_custom(&directory, launch).unwrap();
    let params = json!({"harness":"slow-disk"});

    let idle = prepare_catalog(directory.clone(), "model.list", &params, BTreeSet::new());
    assert!(matches!(
        idle.unwrap(),
        PreparedCatalog::Model { prepared: None, .. }
    ));

    let with_session = BTreeSet::from(["slow-disk".to_owned()]);
    let can_read = std::fs::File::open(&executable).is_ok();
    let live = prepare_catalog(directory.clone(), "model.list", &params, with_session);
    if can_read {
        // Privileged runs (root) read a mode 0o111 file, so the unreadable-file
        // failure cannot be produced here. Say so instead of passing silently.
        eprintln!("skipped: this user reads mode 0o111 files, so hashing cannot fail");
        assert!(live.is_ok(), "a readable executable is hashed");
    } else {
        assert!(live.is_err(), "an unreadable executable cannot be hashed");
    }

    let unknown = prepare_catalog(
        directory.clone(),
        "model.list",
        &json!({"harness":"nope"}),
        BTreeSet::new(),
    );
    assert!(unknown.is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn model_list_for_an_idle_harness_is_not_probed_and_empty() {
    let directory =
        std::env::temp_dir().join(format!("soyeht-model-idle-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let core = Core::open(&directory).unwrap();
    let params = json!({"harness":"registry.claude-acp"});
    let prepared = PreparedCatalog::Model {
        harness: "registry.claude-acp".into(),
        prepared: None,
    };
    let listed = core.agent_catalog_prepared(prepared, &params).unwrap();
    assert_eq!(listed["source"], "not_probed");
    assert_eq!(listed["items"], json!([]));
    drop(core);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn harness_list_names_harnesses_with_the_label_panes_are_named_after() {
    let directory =
        std::env::temp_dir().join(format!("soyeht-harness-label-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let core = Core::open(&directory).unwrap();
    let prepared = prepare_catalog(
        directory.clone(),
        "harness.list",
        &json!({}),
        BTreeSet::new(),
    )
    .unwrap();
    let listed = core.agent_catalog_prepared(prepared, &json!({})).unwrap();
    let entries = registry::catalogue_entries(&directory).unwrap();
    let claude = entries
        .iter()
        .find(|entry| entry.launch.id == "registry.claude-acp")
        .unwrap();
    let item = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "registry.claude-acp")
        .unwrap();
    // The registry calls the adapter "Claude Agent"; panes and pickers say "Claude".
    assert_eq!(claude.launch.name, "Claude Agent");
    assert_eq!(
        item["name"],
        super::super::naming::harness_label(&claude.launch)
    );
    assert_eq!(item["name"], "Claude");
    drop(core);
    std::fs::remove_dir_all(directory).unwrap();
}

/// `harness.list` lists only the checked harnesses, so only a test reaches the catalogue
/// item of one it does not check: the package its first start downloads unchecked, and the
/// stored acknowledgement that ends the notice about it.
#[test]
fn harness_info_shows_an_unacknowledged_registry_package_and_the_acknowledgement_ends_it() {
    let directory =
        std::env::temp_dir().join(format!("soyeht-harness-package-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let mut core = Core::open(&directory).unwrap();
    let entry = registry::builtin_entries()
        .unwrap()
        .into_iter()
        .find(|entry| entry.launch.id == "registry.gemini")
        .unwrap();
    let (package, locked) = super::super::adapter_install::runner_package(&entry.launch).unwrap();
    assert!(!locked);
    // `prepare_catalog` resolves the command on this host's `PATH`, which only decides
    // "available"; the resolved launch is given directly so that the notice under test is
    // exercised on a host without the package runner too.
    let prepared = |entry: &registry::RegistryEntry| PreparedHarness {
        entry: entry.clone(),
        resolved: Some(entry.launch.clone()),
        prepared: None,
        installed_without_success: registry::locally_installed(entry, false),
        installed_with_success: registry::locally_installed(entry, true),
    };

    let before = harness_info(&core, prepared(&entry), false).unwrap();
    assert_eq!(before["package"], json!(&package));
    assert_eq!(before["packageLocked"], json!(false));
    assert_eq!(
        before["setupReason"],
        json!(HarnessSetupReason::RegistryPackageUnverified)
    );

    crate::ports::StorePort::mutate(&mut core.store, "test", &package, &package, |change| {
        soyeht_store::adapter_acknowledgement::acknowledge(change, &package, 1)?;
        Ok((json!({}), Vec::new()))
    })
    .unwrap();

    let after = harness_info(&core, prepared(&entry), false).unwrap();
    assert_eq!(after["package"], json!(&package));
    assert_eq!(after["packageLocked"], json!(false));
    // With the exact package acknowledged, the unacknowledged download no longer is what
    // the item reports: nothing was recorded about this harness yet, so it is only unchecked.
    assert_eq!(
        after["setupReason"],
        json!(HarnessSetupReason::NeverStarted)
    );
    crate::test_paths::remove_after(core, directory);
}

#[test]
fn only_the_four_checked_harnesses_are_supported_and_they_are_the_readiness_table() {
    let supported = registry::builtin_entries()
        .unwrap()
        .into_iter()
        .filter(|entry| entry.support == registry::Support::Supported)
        .map(|entry| entry.launch.id)
        .collect::<BTreeSet<_>>();
    let readiness = soyeht_protocol::readiness::HARNESSES
        .iter()
        .map(|harness| harness.id.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(supported, readiness);
    assert_eq!(supported.len(), 4);
}

#[test]
fn an_unchecked_harness_is_not_listed_for_a_new_pane_but_still_resolves_for_a_bound_one() {
    let directory =
        std::env::temp_dir().join(format!("soyeht-harness-support-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(
        directory.join("harnesses.json"),
        br#"[{"id":"mine","name":"Mine","command":"mine","args":[],"version":"1"}]"#,
    )
    .unwrap();
    let listed = registry::listed_entries(&directory)
        .unwrap()
        .into_iter()
        .map(|entry| entry.launch.id)
        .collect::<BTreeSet<_>>();
    // A custom entry is listed; the unchecked built-in ones are not.
    assert!(listed.contains("mine"));
    assert!(listed.contains("registry.opencode"));
    assert!(!listed.contains("registry.gemini"));
    assert_eq!(listed.len(), 5);
    // Every lookup by id, which a restored pane or a model list uses, reads the whole catalogue.
    let all = registry::entries(&directory).unwrap();
    assert!(all.iter().any(|launch| launch.id == "registry.gemini"));
    assert_eq!(all.len(), registry::builtin_entries().unwrap().len() + 1);
    std::fs::remove_dir_all(directory).unwrap();
}
