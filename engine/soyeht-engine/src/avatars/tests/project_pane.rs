//! Assigning an avatar starts its pane as the workspace's chief agent (ADR 0166 slice 2).

use super::*;
use crate::agents::persona;

/// Gives the workspace the avatar, with a persona, at the revision named.
fn assigned(rig: &mut Rig, workspace: &str, avatar: &Avatar, base: &str, persona: &str) -> Value {
    let extra = json!({"persona": persona});
    rig.assign(workspace, &avatar.avatar_id, base, extra)
        .unwrap()
}

fn pane_of(result: &Value) -> String {
    result["paneId"].as_str().unwrap().to_owned()
}

/// The panes of the workspace that are open.
fn open_panes(rig: &Rig, workspace: &str) -> Vec<String> {
    rig.core
        .panes
        .values()
        .filter(|pane| !pane.stored.closed && pane.stored.pane.workspace_id == workspace)
        .map(|pane| pane.stored.pane.pane_id.clone())
        .collect()
}

#[test]
fn assigning_an_avatar_starts_its_pane_as_the_chief_agent_named_for_it() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let extra = json!({"model": "a-model", "effort": "high"});
    let bound = rig
        .assign(&workspace, &avatar.avatar_id, "0", extra)
        .unwrap();
    let pane = pane_of(&bound);
    assert_eq!(open_panes(&rig, &workspace), vec![pane.clone()]);
    let started = &rig.core.panes[&pane].stored.pane;
    assert_eq!(started.mode, soyeht_protocol::PaneMode::Agent);
    assert_eq!(started.name, "Sabio");
    // In the workspace's own folder, as any pane of it.
    let folder = soyeht_platform::fs::canonicalize(&rig.directory.join("project")).unwrap();
    assert_eq!(Path::new(&started.cwd), folder);
    let agent = &rig.core.agents.get(&pane).unwrap().state;
    assert!(agent.avatar);
    assert_eq!(agent.mcp_role, Role::Chief);
    assert_eq!(agent.launch.id, rig.harness);
    assert_eq!(
        agent.requested_options["model"],
        soyeht_protocol::ConfigValue::V0("a-model".into())
    );
    assert_eq!(
        agent.requested_options["effort"],
        soyeht_protocol::ConfigValue::V0("high".into())
    );
    // The lead prompt of `pane.role.set` is not queued: the persona says what the avatar is for.
    assert!(agent.queue.is_empty());
    // The binding and the pane came with one request: the pane shows in the snapshot of the events.
    let methods: Vec<String> = rig
        .core
        .store
        .replay(0)
        .unwrap()
        .1
        .into_iter()
        .map(|event| event.method)
        .collect();
    let changed = methods.iter().rposition(|m| m == "avatar.changed").unwrap();
    let created = methods.iter().rposition(|m| m == "pane.created").unwrap();
    assert!(changed < created, "{methods:?}");
}

#[test]
fn a_harness_that_cannot_start_leaves_no_binding_and_no_pane() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let refused = rig.assign(
        &workspace,
        &avatar.avatar_id,
        "0",
        json!({"harness": "nobody-has-this"}),
    );
    assert_eq!(kind(&refused), ErrorKind::HarnessUnavailable);
    assert_eq!(
        rig.call("avatar.list", json!({})).unwrap()["bindings"],
        json!([])
    );
    assert!(open_panes(&rig, &workspace).is_empty());
    // Nothing was kept of the refused call: the same revision is still the one to name.
    let bound = rig
        .assign(&workspace, &avatar.avatar_id, "0", json!({}))
        .unwrap();
    assert_eq!(bound["binding"]["revision"], "1");
}

#[test]
fn a_repeated_request_starts_no_second_pane() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let extra = json!({"persona": "Once.", "idempotencyKey": "assign-once"});
    let first = rig
        .assign(&workspace, &avatar.avatar_id, "0", extra.clone())
        .unwrap();
    let second = rig
        .assign(&workspace, &avatar.avatar_id, "0", extra)
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(open_panes(&rig, &workspace).len(), 1);
}

#[test]
fn a_workspace_that_has_its_avatar_pane_open_gets_no_other_and_reads_the_new_persona() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let first = rig
        .assign(
            &workspace,
            &avatar.avatar_id,
            "0",
            json!({"persona": "Version one."}),
        )
        .unwrap();
    let pane = pane_of(&first);
    let state = rig.core.agents.get(&pane).unwrap().state.clone();
    assert_eq!(
        persona::text(&rig.core.store, &state, &workspace)
            .unwrap()
            .unwrap(),
        "You are Sabio, the avatar agent of this project.\n\nVersion one."
    );
    // A new persona is read by the pane the next time its session opens, not by a new pane.
    let second = rig
        .assign(
            &workspace,
            &avatar.avatar_id,
            "1",
            json!({"persona": "Version two."}),
        )
        .unwrap();
    assert_eq!(pane_of(&second), pane);
    assert_eq!(second["binding"]["revision"], "2");
    assert_eq!(open_panes(&rig, &workspace), vec![pane]);
    assert_eq!(
        persona::text(&rig.core.store, &state, &workspace)
            .unwrap()
            .unwrap(),
        "You are Sabio, the avatar agent of this project.\n\nVersion two."
    );
}

#[test]
fn closing_the_avatar_pane_lets_the_next_assignment_start_another() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let first = pane_of(
        &rig.assign(&workspace, &avatar.avatar_id, "0", json!({}))
            .unwrap(),
    );
    rig.core.panes.get_mut(&first).unwrap().stored.closed = true;
    let second = pane_of(
        &rig.assign(&workspace, &avatar.avatar_id, "1", json!({}))
            .unwrap(),
    );
    assert_ne!(first, second);
    assert_eq!(open_panes(&rig, &workspace), vec![second]);
}

#[test]
fn a_pane_that_is_not_an_avatars_has_no_persona() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    assigned(&mut rig, &workspace, &avatar, "0", "Hidden.");
    let ordinary = crate::test_panes::ready_agent_state();
    assert_eq!(
        persona::text(&rig.core.store, &ordinary, &workspace).unwrap(),
        None
    );
}

const PROJECTS: &str = "projects";

#[test]
fn a_project_folder_is_made_under_the_data_directory_and_is_empty() {
    let mut rig = Rig::new();
    let made = rig
        .call("project.folder.create", json!({"name": "Spanish reading"}))
        .unwrap();
    let path = PathBuf::from(made["path"].as_str().unwrap());
    let projects = soyeht_platform::fs::canonicalize(&rig.directory.join(PROJECTS)).unwrap();
    assert_eq!(path, projects.join("spanish-reading"));
    assert!(path.is_dir());
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
    // The same name again is another project, not the same folder.
    let again = rig
        .call("project.folder.create", json!({"name": "Spanish reading"}))
        .unwrap();
    assert_eq!(
        PathBuf::from(again["path"].as_str().unwrap()),
        projects.join("spanish-reading-2")
    );
    // It is a folder a workspace can be made on, and the avatar pane then starts in it.
    let workspace = rig
        .call(
            "workspace.create",
            json!({"name": "Spanish reading", "path": made["path"]}),
        )
        .unwrap();
    let workspace = workspace["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let avatar = rig.avatar("Sabio");
    let pane = pane_of(
        &rig.assign(&workspace, &avatar.avatar_id, "0", json!({}))
            .unwrap(),
    );
    assert_eq!(Path::new(&rig.core.panes[&pane].stored.pane.cwd), path);
}

#[cfg(unix)]
#[test]
fn the_projects_folder_and_a_project_are_private_to_the_owner() {
    use std::os::unix::fs::PermissionsExt;
    let mut rig = Rig::new();
    let made = rig
        .call("project.folder.create", json!({"name": "Private"}))
        .unwrap();
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(Path::new(made["path"].as_str().unwrap())), 0o700);
    assert_eq!(mode(&rig.directory.join(PROJECTS)), 0o700);
}

#[test]
fn a_retry_with_the_same_key_makes_no_second_folder() {
    let mut rig = Rig::new();
    let params = json!({"name": "Once", "idempotencyKey": "same-key"});
    let first = rig.call("project.folder.create", params.clone()).unwrap();
    let second = rig.call("project.folder.create", params).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        std::fs::read_dir(rig.directory.join(PROJECTS))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn a_name_with_nothing_in_it_or_too_long_is_refused_and_makes_nothing() {
    let mut rig = Rig::new();
    for name in [
        String::new(),
        "  ".to_owned(),
        "x".repeat(limits::PROJECT_NAME_CHARS + 1),
    ] {
        let refused = rig.call("project.folder.create", json!({ "name": name }));
        assert_eq!(kind(&refused), ErrorKind::InvalidParams);
    }
    assert!(!rig.directory.join(PROJECTS).exists());
}

#[test]
fn an_agent_and_a_chief_cannot_make_a_project_folder() {
    let mut rig = Rig::new();
    for role in [Role::Agent, Role::Chief] {
        let connection = rig.connect(role, None);
        let refused = rig.call_as(&connection, "project.folder.create", json!({"name": "X"}));
        assert_eq!(kind(&refused), ErrorKind::ForbiddenRole);
    }
    assert!(!rig.directory.join(PROJECTS).exists());
}
