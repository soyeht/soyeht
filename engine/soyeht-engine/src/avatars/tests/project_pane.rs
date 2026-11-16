//! Assigning an avatar starts its pane as the workspace's chief agent (ADR 0166 slice 2).

use super::*;
use crate::agents::persona;

/// Gives the workspace the avatar, with a persona, at the revision named.
fn assigned(rig: &mut Rig, workspace: &str, avatar: &Avatar, base: &str, persona: &str) -> Value {
    let extra = json!({"persona": persona});
    rig.assign(workspace, &avatar.avatar_id, base, extra)
        .unwrap()
}

/// A rig with the avatar `Sabio` and an open workspace named `project`.
fn project() -> (Rig, Avatar, String) {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    (rig, avatar, workspace)
}

/// The pane the first assignment of the avatar to the workspace starts.
fn started(rig: &mut Rig, workspace: &str, avatar: &Avatar) -> String {
    pane_of(&assigned(rig, workspace, avatar, "0", ""))
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
    let (mut rig, avatar, workspace) = project();
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
    let (mut rig, avatar, workspace) = project();
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
    let (mut rig, avatar, workspace) = project();
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

/// The request a session of the pane would open with, the way the engine opens it: the pane's
/// state as it is now, for an adapter that takes a system prompt.
fn session_request(rig: &Rig, pane: &str) -> Value {
    use crate::agents::{Changes, responses};
    let mut state = rig.core.agent_state(pane).unwrap();
    state.mcp_enabled = false;
    state.session_id = Some("native-1".into());
    state.initialized = Some(crate::test_panes::initialization(true));
    let runtime = responses::Runtime {
        data_directory: &rig.directory,
        now_ms: 0,
        timestamp: "now",
    };
    let mut changes = Changes::default();
    let saved = rig.core.panes[pane].stored.clone();
    responses::open(&rig.core.store, &runtime, &mut state, &saved, &mut changes).unwrap();
    match changes.outgoing.as_slice() {
        [soyeht_acp::Message::Request { params, .. }] => params.clone(),
        other => panic!("expected one request, got {other:?}"),
    }
}

fn system_prompt_text(request: &Value) -> &str {
    request["systemPrompt"]["content"][0]["text"]
        .as_str()
        .unwrap()
}

#[test]
fn a_workspace_that_has_its_avatar_pane_open_gets_no_other_and_its_session_reads_the_new_persona() {
    let (mut rig, avatar, workspace) = project();
    let first = rig
        .assign(
            &workspace,
            &avatar.avatar_id,
            "0",
            json!({"persona": "Version one."}),
        )
        .unwrap();
    let pane = pane_of(&first);
    assert!(system_prompt_text(&session_request(&rig, &pane)).ends_with("Version one."));
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
    assert_eq!(open_panes(&rig, &workspace), vec![pane.clone()]);
    assert!(system_prompt_text(&session_request(&rig, &pane)).ends_with("Version two."));
}

#[test]
fn a_changed_persona_is_owed_to_the_next_prompt_of_a_harness_with_no_system_prompt() {
    let (mut rig, avatar, workspace) = project();
    let pane = started(&mut rig, &workspace, &avatar);
    // The pane's adapter has not announced a system prompt: it never does in the rig.
    assert!(!rig.core.agent_state(&pane).unwrap().persona_pending);
    rig.assign(
        &workspace,
        &avatar.avatar_id,
        "1",
        json!({"persona": "Changed."}),
    )
    .unwrap();
    assert!(rig.core.agent_state(&pane).unwrap().persona_pending);
    // The same text again changes nothing the harness has to be told.
    rig.core
        .agents
        .get_mut(&pane)
        .unwrap()
        .state
        .persona_pending = false;
    rig.assign(
        &workspace,
        &avatar.avatar_id,
        "2",
        json!({"persona": "Changed."}),
    )
    .unwrap();
    assert!(!rig.core.agent_state(&pane).unwrap().persona_pending);
}

#[test]
fn an_assignment_names_a_harness_the_profile_has_whether_or_not_a_pane_is_open() {
    let (mut rig, avatar, workspace) = project();
    assigned(&mut rig, &workspace, &avatar, "0", "");
    let refused = rig.assign(
        &workspace,
        &avatar.avatar_id,
        "1",
        json!({"harness": "nobody-has-this"}),
    );
    assert_eq!(kind(&refused), ErrorKind::HarnessUnavailable);
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["bindings"][0]["revision"], "1");
    assert_eq!(listed["bindings"][0]["harness"], rig.harness);
}

#[test]
fn a_changed_harness_switches_the_open_pane_and_the_binding_keeps_what_the_pane_has_until_it_ends()
{
    let (mut rig, avatar, workspace) = project();
    rig.add_harness("avatar-other");
    let pane = started(&mut rig, &workspace, &avatar);
    let switching = rig
        .assign(
            &workspace,
            &avatar.avatar_id,
            "1",
            json!({"harness": "avatar-other", "model": "m2", "effort": "high"}),
        )
        .unwrap();
    assert_eq!(pane_of(&switching), pane);
    let checkpoint = rig.core.agent_state(&pane).unwrap().harness_switch.unwrap();
    assert_eq!(checkpoint.to_harness, "avatar-other");
    assert_eq!(checkpoint.model.as_deref(), Some("m2"));
    assert_eq!(checkpoint.effort.as_deref(), Some("high"));
    // The pane is on the harness it had, and so is the binding: the end of the switch records the
    // new one (a test of the commit is in `switching`), and a switch that fails never does.
    assert_eq!(switching["binding"]["harness"], rig.harness);
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["bindings"][0]["harness"], rig.harness);
    assert_eq!(listed["bindings"][0]["revision"], "2");
}

/// The announcement of a session that offers a model and an effort.
fn announce(rig: &mut Rig, pane: &str, model: &str, effort: &str) {
    let option = |id: &str, category: &str, current: &str| {
        json!({"id": id, "name": id, "category": category, "type": "select",
            "currentValue": current,
            "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"}]})
    };
    let options = soyeht_acp::config::Options::from_response(&json!({"configOptions": [
        option("model", "model", model), option("effort", "thought_level", effort)]}))
    .unwrap();
    rig.core.agents.get_mut(pane).unwrap().state.options = options;
}

#[test]
fn a_changed_model_and_effort_are_both_asked_for_in_one_change() {
    use soyeht_protocol::ConfigValue::V0;
    let (mut rig, avatar, workspace) = project();
    let pane = started(&mut rig, &workspace, &avatar);
    announce(&mut rig, &pane, "a", "a");
    let wanted = json!({"model": "b", "effort": "b"});
    let changed = rig
        .assign(&workspace, &avatar.avatar_id, "1", wanted)
        .unwrap();
    assert_eq!(pane_of(&changed), pane);
    let state = rig.core.agent_state(&pane).unwrap();
    // Both wait for the pane to be idle, and the engine applies them one at a time.
    assert_eq!(state.requested_options["model"], V0("b".into()));
    assert_eq!(state.requested_options["effort"], V0("b".into()));
    assert!(state.operation.is_none() && state.needs_configuration());
    // What the session has already is not asked for again.
    rig.core
        .agents
        .get_mut(&pane)
        .unwrap()
        .state
        .requested_options
        .clear();
    let same = json!({"model": "a", "effort": "b"});
    rig.assign(&workspace, &avatar.avatar_id, "2", same)
        .unwrap();
    let state = rig.core.agent_state(&pane).unwrap();
    assert!(!state.requested_options.contains_key("model"));
    assert_eq!(state.requested_options["effort"], V0("b".into()));
}

#[test]
fn a_changed_persona_is_owed_to_a_pane_whose_adapter_has_a_system_prompt_too() {
    let (mut rig, avatar, workspace) = project();
    let pane = started(&mut rig, &workspace, &avatar);
    rig.core.agents.get_mut(&pane).unwrap().state.initialized =
        Some(crate::test_panes::initialization(true));
    assert_eq!(
        system_prompt_text(&session_request(&rig, &pane)),
        "You are Sabio, the avatar agent of this project."
    );
    assigned(&mut rig, &workspace, &avatar, "1", "Changed.");
    // The next prompt carries it, and the system prompt catches up when the session next opens.
    assert!(rig.core.agent_state(&pane).unwrap().persona_pending);
    assert!(system_prompt_text(&session_request(&rig, &pane)).ends_with("Changed."));
}

#[test]
fn a_pane_that_cannot_be_switched_is_replaced_when_another_harness_is_asked_for() {
    use crate::agents::state::Phase;
    let (mut rig, avatar, workspace) = project();
    rig.add_harness("avatar-other");
    let stuck = started(&mut rig, &workspace, &avatar);
    rig.core.agents.get_mut(&stuck).unwrap().state.phase = Phase::NeedsLogin;
    // The same harness stays: the person signs in to it.
    assigned(&mut rig, &workspace, &avatar, "1", "");
    assert_eq!(open_panes(&rig, &workspace), vec![stuck.clone()]);
    let other = json!({"harness": "avatar-other"});
    let moved = rig
        .assign(&workspace, &avatar.avatar_id, "2", other)
        .unwrap();
    let new = pane_of(&moved);
    assert_ne!(new, stuck);
    assert_eq!(open_panes(&rig, &workspace), vec![new.clone()]);
    assert_eq!(moved["binding"]["harness"], "avatar-other");
    assert_eq!(
        rig.core.agent_state(&new).unwrap().launch.id,
        "avatar-other"
    );
}

#[test]
fn a_pane_that_failed_for_good_is_closed_when_the_next_assignment_starts_another() {
    let (mut rig, avatar, workspace) = project();
    let dead = started(&mut rig, &workspace, &avatar);
    rig.core.agents.get_mut(&dead).unwrap().state.phase = crate::agents::state::Phase::Faulted;
    let next = pane_of(&assigned(&mut rig, &workspace, &avatar, "1", ""));
    assert_ne!(next, dead);
    // A workspace has one avatar pane, and it is the one that works.
    assert_eq!(open_panes(&rig, &workspace), vec![next.clone()]);
    assert_eq!(
        pane_of(&assigned(&mut rig, &workspace, &avatar, "2", "")),
        next
    );
}

#[test]
fn the_pane_is_named_for_the_avatar_after_a_rename_and_after_another_avatar_is_given() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let other = rig.avatar("Mentor");
    let workspace = rig.workspace("project");
    let pane = started(&mut rig, &workspace, &avatar);
    let name = |rig: &Rig| rig.core.panes[&pane].stored.pane.name.clone();
    assert_eq!(name(&rig), "Sabio");
    rig.call(
        "avatar.rename",
        json!({"avatarId": avatar.avatar_id, "name": "Sage"}),
    )
    .unwrap();
    assert_eq!(name(&rig), "Sage");
    rig.assign(&workspace, &other.avatar_id, "1", json!({}))
        .unwrap();
    assert_eq!(name(&rig), "Mentor");
}

#[test]
fn closing_the_avatar_pane_lets_the_next_assignment_start_another() {
    let (mut rig, avatar, workspace) = project();
    let first = started(&mut rig, &workspace, &avatar);
    rig.call("pane.close", json!({"paneId": first})).unwrap();
    assert!(open_panes(&rig, &workspace).is_empty());
    let second = pane_of(
        &rig.assign(&workspace, &avatar.avatar_id, "1", json!({}))
            .unwrap(),
    );
    assert_ne!(first, second);
    assert_eq!(open_panes(&rig, &workspace), vec![second]);
}

#[test]
fn a_pane_that_is_not_an_avatars_has_no_persona() {
    let (mut rig, avatar, workspace) = project();
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
    let pane = started(&mut rig, &workspace, &avatar);
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

/// The announcement of a session that offers a model and no effort.
fn announce_model_only(rig: &mut Rig, pane: &str, model: &str) {
    let options = soyeht_acp::config::Options::from_response(&json!({"configOptions": [
        {"id": "model", "name": "model", "category": "model", "type": "select",
         "currentValue": model, "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"}]}]}))
    .unwrap();
    rig.core.agents.get_mut(pane).unwrap().state.options = options;
}

#[test]
fn a_selector_the_session_does_not_offer_is_not_asked_for_and_the_pane_keeps_taking_prompts() {
    let (mut rig, avatar, workspace) = project();
    let pane = started(&mut rig, &workspace, &avatar);
    // Nothing is announced yet: nothing is asked for, so nothing holds the queue back.
    let wanted = json!({"model": "b", "effort": "high"});
    rig.assign(&workspace, &avatar.avatar_id, "1", wanted).unwrap();
    assert!(!rig.core.agent_state(&pane).unwrap().needs_configuration());
    // The model the session has now offers no effort: the effort stays in the binding only.
    announce(&mut rig, &pane, "a", "a");
    rig.assign(&workspace, &avatar.avatar_id, "2", json!({"model": "b", "effort": "a"}))
        .unwrap();
    assert_eq!(rig.core.agent_state(&pane).unwrap().requested_options.len(), 1);
    rig.core.agents.get_mut(&pane).unwrap().state.requested_options.clear();
    announce_model_only(&mut rig, &pane, "b");
    let moved = json!({"model": "b", "effort": "low"});
    let answered = rig.assign(&workspace, &avatar.avatar_id, "3", moved).unwrap();
    assert_eq!(answered["binding"]["effort"], "low");
    assert!(!rig.core.agent_state(&pane).unwrap().needs_configuration());
}

#[test]
fn a_pane_that_is_coming_back_is_not_replaced_and_the_switch_says_it_is_busy() {
    use crate::agents::state::Phase;
    let (mut rig, avatar, workspace) = project();
    rig.add_harness("avatar-other");
    let pane = started(&mut rig, &workspace, &avatar);
    rig.core.agents.get_mut(&pane).unwrap().state.phase = Phase::Recovering;
    let other = json!({"harness": "avatar-other"});
    let busy = rig.assign(&workspace, &avatar.avatar_id, "1", other);
    assert_eq!(kind(&busy), ErrorKind::PaneBusy);
    assert_eq!(open_panes(&rig, &workspace), vec![pane]);
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["bindings"][0]["revision"], "1");
}

/// Queues `text` on the pane, durably.
fn queue(rig: &mut Rig, pane: &str, text: &str) -> String {
    let mut state = rig.core.agent_state(pane).unwrap();
    let item = state
        .enqueue(text.into(), soyeht_protocol::PromptOrigin::Human, "t".into())
        .unwrap();
    let saved = rig.core.panes[pane].stored.clone();
    rig.core
        .commit_agent(saved, state, crate::agents::Changes::default(), None, json!({}))
        .unwrap();
    item.prompt_id
}

#[test]
fn a_replaced_pane_gives_its_queued_prompts_to_the_new_one_in_order() {
    use crate::agents::state::Phase;
    let (mut rig, avatar, workspace) = project();
    rig.add_harness("avatar-other");
    let stuck = started(&mut rig, &workspace, &avatar);
    let ids = [queue(&mut rig, &stuck, "first"), queue(&mut rig, &stuck, "second")];
    rig.core.agents.get_mut(&stuck).unwrap().state.phase = Phase::NeedsLogin;
    let other = json!({"harness": "avatar-other"});
    let new = pane_of(&rig.assign(&workspace, &avatar.avatar_id, "1", other).unwrap());
    let carried: Vec<String> = rig
        .core
        .agent_state(&new)
        .unwrap()
        .queue
        .iter()
        .map(|prompt| prompt.prompt_id.clone())
        .collect();
    assert_eq!(carried, ids);
    // The rows of the store follow: the new pane has them, the closed one does not.
    let stored = |rig: &Rig, pane: &str| -> Vec<String> {
        soyeht_store::agents::queue(&rig.core.store, pane)
            .unwrap()
            .into_iter()
            .map(|prompt| prompt.prompt_id)
            .collect()
    };
    assert_eq!(stored(&rig, &new), ids);
    assert!(stored(&rig, &stuck).is_empty());
}

#[cfg(unix)]
#[test]
fn a_replacement_that_is_refused_leaves_the_pane_and_the_binding_as_they_were() {
    use crate::agents::state::Phase;
    use std::os::unix::fs::PermissionsExt;
    let (mut rig, avatar, workspace) = project();
    let stuck = started(&mut rig, &workspace, &avatar);
    rig.core.agents.get_mut(&stuck).unwrap().state.phase = Phase::NeedsLogin;
    // A harness that can be resolved (it runs) and not started (its program cannot be read).
    let program = rig.directory.join("unreadable-agent");
    std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o111)).unwrap();
    if std::fs::read(&program).is_ok() {
        return; // The tests run as a user that reads everything.
    }
    rig.call(
        "harness.custom.create",
        json!({"harness": {"id": "avatar-unreadable", "name": "x",
            "command": program.to_string_lossy(), "args": [], "version": "1"}}),
    )
    .unwrap();
    let refused = rig.assign(
        &workspace,
        &avatar.avatar_id,
        "1",
        json!({"harness": "avatar-unreadable"}),
    );
    assert!(refused.is_err());
    assert_eq!(open_panes(&rig, &workspace), vec![stuck]);
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["bindings"][0]["revision"], "1");
    assert_eq!(listed["bindings"][0]["harness"], rig.harness);
}

#[test]
fn a_changed_persona_is_owed_while_a_switch_runs_too() {
    let (mut rig, avatar, workspace) = project();
    rig.add_harness("avatar-other");
    let pane = started(&mut rig, &workspace, &avatar);
    let both = json!({"harness": "avatar-other", "persona": "New persona."});
    rig.assign(&workspace, &avatar.avatar_id, "1", both).unwrap();
    let state = rig.core.agent_state(&pane).unwrap();
    // If the switch fails the pane stays on its harness, and its next prompt carries the new persona.
    assert!(state.harness_switch.is_some() && state.persona_pending);
}
