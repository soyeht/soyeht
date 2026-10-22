use super::*;
use crate::{
    Event,
    testing::{cases, pane, text, workspace},
};
use serde_json::{Value, json};

fn state_with(workspaces: &[(&str, &str)]) -> State {
    State {
        workspaces: workspaces
            .iter()
            .map(|(id, path)| {
                let mut made = workspace(id);
                made.path = (*path).to_owned();
                made
            })
            .collect(),
        ..State::default()
    }
}

fn held(case: &Value) -> Vec<(&str, &str)> {
    case["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|held| (text(&held["workspaceId"]), text(&held["path"])))
        .collect()
}

#[test]
fn a_workspace_is_named_for_the_last_component_of_its_folder_as_the_vector_says() {
    for case in cases("workspaces", "names") {
        assert_eq!(
            name_of(text(&case["path"])),
            text(&case["workspaceName"]),
            "{}",
            text(&case["name"])
        );
    }
}

#[test]
fn a_typed_path_is_resolved_as_the_vector_says() {
    for case in cases("workspaces", "resolve") {
        let resolved = resolve_path(text(&case["typed"]));
        match case["problem"].as_str() {
            None => assert_eq!(
                resolved.as_deref(),
                Ok(text(&case["path"])),
                "{}",
                text(&case["name"])
            ),
            Some(problem) => assert_eq!(
                resolved,
                Err(match problem {
                    "empty" => FolderPathProblem::Empty,
                    _ => FolderPathProblem::NotAbsolute,
                }),
                "{}",
                text(&case["name"])
            ),
        }
    }
}

#[test]
fn a_folder_that_has_a_workspace_reuses_it_as_the_vector_says() {
    for case in cases("workspaces", "reuse") {
        let opening = state_with(&held(&case)).open_folder(text(&case["path"]));
        let reused = match opening {
            Ok(FolderOpening::Existing(id)) => Some(id),
            _ => None,
        };
        assert_eq!(
            reused.as_deref(),
            case["reuses"].as_str(),
            "{}",
            text(&case["name"])
        );
    }
}

#[test]
fn the_recent_folders_are_the_ones_the_vector_lists() {
    for case in cases("workspaces", "recent") {
        let mut state = state_with(&held(&case));
        state.panes = case["paneFolders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|folder| {
                let mut made = pane("p", "w1", "terminal");
                made.cwd = text(folder).to_owned();
                made
            })
            .collect();
        let listed: Vec<Value> = state
            .recent_folders()
            .into_iter()
            .map(|folder| {
                json!({"path": folder.path, "name": folder.name, "workspaceId": folder.workspace_id})
            })
            .collect();
        assert_eq!(
            &Value::Array(listed),
            &case["folders"],
            "{}",
            text(&case["name"])
        );
    }
}

#[test]
fn a_rename_is_submitted_as_the_vector_says() {
    for case in cases("workspaces", "submission") {
        assert_eq!(
            submission(text(&case["typed"]), text(&case["current"])).as_deref(),
            case["submitted"].as_str(),
            "{}",
            text(&case["name"])
        );
    }
}

#[test]
fn closing_a_workspace_selects_the_one_the_vector_says() {
    for case in cases("workspaces", "closing") {
        let order: Vec<String> = case["order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| text(id).to_owned())
            .collect();
        assert_eq!(
            selection_after_closing(&order, text(&case["closed"])).as_deref(),
            case["selected"].as_str(),
            "{}",
            text(&case["name"])
        );
    }
}

#[test]
fn closing_the_selected_workspace_moves_the_selection_to_its_neighbour() {
    let mut state = state_with(&[("a", "/a"), ("b", "/b"), ("c", "/c")]);
    state.select_workspace("b");
    state.close_workspace("b");
    assert_eq!(state.selected_workspace_id().as_deref(), Some("c"));
    state.close_workspace("c");
    assert_eq!(state.selected_workspace_id().as_deref(), Some("a"));
    state.close_workspace("a");
    assert_eq!(state.selected_workspace_id(), None);
}

#[test]
fn closing_another_workspace_keeps_the_selection() {
    let mut state = state_with(&[("a", "/a"), ("b", "/b")]);
    state.select_workspace("b");
    state.close_workspace("a");
    assert_eq!(state.selected_workspace_id().as_deref(), Some("b"));
}

#[test]
fn a_chosen_folder_without_a_workspace_is_named_for_its_last_component() {
    let state = state_with(&[("a", "/a")]);
    assert_eq!(
        state.open_folder("/home/ana/notes/"),
        Ok(FolderOpening::New {
            name: "notes".into(),
            path: "/home/ana/notes".into()
        })
    );
    assert_eq!(
        state.open_folder("notes"),
        Err(FolderPathProblem::NotAbsolute)
    );
}

#[test]
fn the_workspace_the_engine_made_is_selected_and_gets_a_terminal_in_its_folder() {
    let mut state = state_with(&[("a", "/a")]);
    let mut made = workspace("new");
    made.path = "/home/ana/notes".to_owned();
    let effects = state.apply(Event::WorkspaceOpened {
        ticket: 5,
        workspace: made,
    });
    assert_eq!(state.selected_workspace_id().as_deref(), Some("new"));
    assert_eq!(
        effects,
        [Effect::Send(Command::CreateTerminal {
            workspace_id: "new".into(),
            cwd: Some("/home/ana/notes".into()),
            ticket: 5,
        })]
    );
    // The pane that answers the ticket takes the keyboard.
    let effects = state.apply(Event::Opened {
        ticket: 5,
        pane_id: "p".into(),
    });
    assert!(effects.is_empty(), "the pane has not arrived yet");
    let arrival = crate::testing::event(
        soyeht_protocol::EventName::PaneCreated,
        json!({"pane": crate::testing::pane_json("p", "new", "terminal")}),
    );
    assert!(
        state
            .apply(arrival)
            .contains(&Effect::FocusPane("p".into()))
    );
}

fn with_agent(state: &mut State) {
    let agent: soyeht_protocol::HarnessInfo = serde_json::from_value(json!({
        "id": "ada", "name": "Ada", "available": true, "capabilities": [], "options": [],
        "authMethods": [], "installed": true, "setupState": "ready"
    }))
    .unwrap();
    state.apply(Event::Harnesses(vec![agent]));
}

/// The agent pane `ticket` asked for arrives in workspace `workspace_id`: the effects it brings.
fn agent_arrives(state: &mut State, ticket: u64, pane_id: &str, workspace_id: &str) -> Vec<Effect> {
    state.apply(Event::Opened {
        ticket,
        pane_id: pane_id.into(),
    });
    let arrival = crate::testing::event(
        soyeht_protocol::EventName::PaneCreated,
        json!({"pane": crate::testing::pane_json(pane_id, workspace_id, "agent")}),
    );
    state.apply(arrival)
}

fn sent_prompt(effects: &[Effect]) -> Option<&str> {
    effects.iter().find_map(|effect| match effect {
        Effect::Send(Command::Prompt { text, .. }) => Some(text.as_str()),
        _ => None,
    })
}

fn form(directory: &str, prompt: &str) -> NewAgent {
    NewAgent {
        directory: directory.to_owned(),
        harness: "ada".to_owned(),
        prompt: prompt.to_owned(),
    }
}

#[test]
fn an_agent_started_in_a_new_folder_gets_its_workspace_first_and_then_its_first_message() {
    let mut state = state_with(&[("a", "/a")]);
    with_agent(&mut state);
    let started = state
        .start_agent(7, &form("/home/ana/notes/", "  hello\n"))
        .unwrap();
    assert_eq!(started.select, None);
    assert_eq!(
        started.effects,
        [Effect::Send(Command::CreateWorkspace {
            name: "notes".into(),
            path: "/home/ana/notes".into(),
            ticket: 7,
        })]
    );
    let mut made = workspace("new");
    made.path = "/home/ana/notes".to_owned();
    let effects = state.apply(Event::WorkspaceOpened {
        ticket: 7,
        workspace: made,
    });
    assert_eq!(
        effects,
        [Effect::Send(Command::CreateAgent {
            workspace_id: "new".into(),
            harness: "ada".into(),
            cwd: Some("/home/ana/notes".into()),
            ticket: 7,
        })]
    );
    let effects = agent_arrives(&mut state, 7, "p", "new");
    assert!(effects.contains(&Effect::FocusPane("p".into())));
    assert_eq!(sent_prompt(&effects), Some("hello"));
}

#[test]
fn an_agent_started_in_a_folder_that_has_a_workspace_uses_it_and_a_blank_message_sends_nothing() {
    let mut state = state_with(&[("a", "/a")]);
    with_agent(&mut state);
    let started = state.start_agent(3, &form("/a", "   ")).unwrap();
    assert_eq!(started.select.as_deref(), Some("a"));
    assert_eq!(
        started.effects,
        [Effect::Send(Command::CreateAgent {
            workspace_id: "a".into(),
            harness: "ada".into(),
            cwd: None,
            ticket: 3,
        })]
    );
    let effects = agent_arrives(&mut state, 3, "p", "a");
    assert!(effects.contains(&Effect::FocusPane("p".into())));
    assert_eq!(sent_prompt(&effects), None);
}

#[test]
fn an_agent_that_the_form_does_not_allow_does_not_start() {
    let mut state = state_with(&[("a", "/a")]);
    with_agent(&mut state);
    assert_eq!(
        state.start_agent(1, &form("", "hi")),
        Err("Choose a folder.")
    );
    assert_eq!(
        state.start_agent(1, &form("a/b", "hi")),
        Err("Use the full path from the top of the disk, such as /home/you/project.")
    );
    let mut no_agent = form("/a", "hi");
    no_agent.harness.clear();
    assert_eq!(state.start_agent(1, &no_agent), Err("Choose an agent."));
    assert!(state.expected.is_empty() && state.starts.is_empty());
}

#[test]
fn a_workspace_that_could_not_be_made_forgets_the_agent_waiting_for_it() {
    let mut state = state_with(&[]);
    with_agent(&mut state);
    state.start_agent(9, &form("/new", "hi")).unwrap();
    state.apply(Event::OpenFailed { ticket: 9 });
    assert!(state.starts.is_empty());
    // What the person typed comes back, once.
    assert_eq!(state.take_failed_start(), Some(form("/new", "hi")));
    assert_eq!(state.take_failed_start(), None);
}

#[test]
fn an_agent_that_could_not_be_created_in_its_workspace_comes_back_with_its_message() {
    let mut state = state_with(&[("a", "/a")]);
    with_agent(&mut state);
    state.start_agent(4, &form("/a", "make it")).unwrap();
    state.apply(Event::OpenFailed { ticket: 4 });
    assert!(state.expected.is_empty());
    assert_eq!(state.take_failed_start(), Some(form("/a", "make it")));
}

#[test]
fn a_connection_lost_while_an_agent_starts_keeps_its_message_for_the_form() {
    let mut state = state_with(&[]);
    with_agent(&mut state);
    state.start_agent(6, &form("/new", "keep me")).unwrap();
    state.apply(Event::Disconnected("gone".into()));
    assert!(state.starts.is_empty() && state.expected.is_empty());
    assert_eq!(state.take_failed_start(), Some(form("/new", "keep me")));
}
