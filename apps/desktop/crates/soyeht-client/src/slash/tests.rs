use super::*;
use crate::{
    Effect, Event, State,
    snapshot::Snapshot,
    testing::{agent_pane, cases, event, pane_json, text, vector, workspace},
};
use serde_json::{Value, json};
use soyeht_protocol::EventName;

/// The skills and commands of a named catalog of `contracts/vectors/slash-completion.json`.
fn catalog_of(name: &str) -> (Vec<Skill>, Vec<AgentCommand>) {
    let entry = &vector("slash-completion")["catalogs"][name];
    (
        serde_json::from_value(entry["skills"].clone()).unwrap(),
        serde_json::from_value(entry["commands"].clone()).unwrap(),
    )
}

fn with_catalog<T>(name: &str, check: impl FnOnce(Catalog) -> T) -> T {
    let (skills, commands) = catalog_of(name);
    check(Catalog::new(&skills, &commands))
}

fn source_of(name: &str) -> Source {
    match name {
        "user" => Source::Skill(SkillScope::User),
        "workspace" => Source::Skill(SkillScope::Workspace),
        _ => Source::Command,
    }
}

fn names(suggestions: &[Suggestion]) -> Vec<&str> {
    suggestions.iter().map(|s| s.name.as_str()).collect()
}

fn wanted(case: &Value) -> Vec<&str> {
    case["names"].as_array().unwrap().iter().map(text).collect()
}

#[test]
fn every_shared_case_offers_the_same_suggestions_in_the_same_order() {
    let all = cases("slash-completion", "suggestions");
    assert!(all.len() >= 2);
    for case in all {
        let found = with_catalog(text(&case["catalog"]), |catalog| catalog.suggestions());
        let expected: Vec<Suggestion> = case["expected"]
            .as_array()
            .unwrap()
            .iter()
            .map(|want| Suggestion {
                name: text(&want["name"]).to_owned(),
                detail: text(&want["detail"]).to_owned(),
                hint: want["hint"].as_str().map(str::to_owned),
                source: source_of(text(&want["source"])),
            })
            .collect();
        assert_eq!(found, expected, "{}", case["name"]);
    }
}

#[test]
fn every_shared_case_reads_the_query_the_same_way() {
    let all = cases("slash-completion", "queries");
    assert!(all.len() >= 8);
    for case in all {
        assert_eq!(
            query(text(&case["text"])),
            case["query"].as_str(),
            "{}",
            case["text"]
        );
    }
}

#[test]
fn every_shared_case_filters_and_orders_the_same_way() {
    let all = cases("slash-completion", "matches");
    assert!(all.len() >= 7);
    for case in all {
        let found = with_catalog(text(&case["catalog"]), |catalog| {
            catalog.matches(text(&case["query"]))
        });
        assert_eq!(names(&found), wanted(&case), "{}", case["name"]);
    }
}

#[test]
fn every_shared_case_leaves_the_same_text_when_an_entry_is_chosen() {
    for case in cases("slash-completion", "accepts") {
        let suggestion = Suggestion {
            name: text(&case["name"]).to_owned(),
            detail: String::new(),
            hint: None,
            source: source_of(text(&case["source"])),
        };
        assert_eq!(accept(&suggestion), text(&case["text"]));
    }
}

#[test]
fn every_shared_case_shows_the_same_rows() {
    assert_eq!(
        VISIBLE_ROWS as u64,
        vector("slash-completion")["visibleRows"].as_u64().unwrap()
    );
    for case in cases("slash-completion", "shown") {
        let rows = with_catalog(text(&case["catalog"]), |catalog| {
            List::default().rows(&catalog, text(&case["text"]))
        });
        assert_eq!(names(&rows), wanted(&case), "{}", case["name"]);
    }
}

#[test]
fn every_shared_case_sends_the_same_prompt() {
    let all = cases("slash-completion", "prompts");
    assert!(all.len() >= 11);
    for case in all {
        let sent = with_catalog(text(&case["catalog"]), |catalog| {
            catalog.prompt(text(&case["text"]))
        });
        assert_eq!(sent, text(&case["prompt"]), "{}", case["text"]);
    }
}

#[test]
fn a_windows_skill_folder_is_read_through_its_own_separator() {
    assert_eq!(
        manifest(r"C:\Users\me\skills\notes"),
        r"C:\Users\me\skills\notes\SKILL.md"
    );
    assert_eq!(
        manifest("/home/me/skills/notes/"),
        "/home/me/skills/notes/SKILL.md"
    );
}

#[test]
fn the_choice_wraps_and_goes_back_to_the_first_row_when_the_text_changes() {
    with_catalog("many", |catalog| {
        let mut list = List::default();
        let rows = list.rows(&catalog, "/");
        assert_eq!(list.selected(&rows), Some(0));
        list.step(-1, rows.len());
        assert_eq!(list.selected(&rows), Some(VISIBLE_ROWS - 1));
        list.step(1, rows.len());
        assert_eq!(list.selected(&rows), Some(0));
        list.step(2, rows.len());
        assert_eq!(list.selected(&rows), Some(2));
        let narrowed = list.rows(&catalog, "/e");
        assert_eq!(list.selected(&narrowed), Some(0));
        assert_eq!(list.selected(&[]), None);
    });
}

#[test]
fn escape_closes_the_list_until_the_query_changes() {
    with_catalog("many", |catalog| {
        let mut list = List::default();
        assert!(!list.rows(&catalog, "/ei").is_empty());
        list.dismiss("/ei");
        assert!(list.rows(&catalog, "/ei").is_empty());
        assert!(!list.rows(&catalog, "/e").is_empty());
        assert!(!list.rows(&catalog, "/ei").is_empty());
    });
}

fn state_with_agent(workspace_id: &str) -> State {
    let mut state = State::default();
    state.apply(Event::Connected(Snapshot {
        workspaces: vec![workspace(workspace_id)],
        panes: vec![agent_pane("a", workspace_id, json!({}))],
        ..Snapshot::default()
    }));
    state
}

fn skill_listing(items: Value) -> soyeht_protocol::SkillsListResult {
    serde_json::from_value(json!({"items": items, "userDirectory": "/skills"})).unwrap()
}

fn skill(name: &str, scope: &str) -> Value {
    json!({"name": name, "scope": scope, "path": format!("/skills/{name}"), "description": "Does things."})
}

#[test]
fn a_pane_lists_the_skills_of_its_workspace_and_falls_back_to_the_users() {
    let mut state = state_with_agent("w");
    state.apply(Event::Skills {
        workspace_id: None,
        listing: skill_listing(json!([skill("mine", "user")])),
    });
    state.apply(Event::PaneCommands {
        pane_id: "a".into(),
        items: serde_json::from_value(json!([{"name": "compact", "description": "Built in."}]))
            .unwrap(),
    });
    assert_eq!(
        names(&state.slash_catalog("a").suggestions()),
        ["mine", "compact"]
    );
    state.apply(Event::Skills {
        workspace_id: Some("w".into()),
        listing: skill_listing(json!([skill("mine", "user"), skill("ours", "workspace")])),
    });
    assert_eq!(
        names(&state.slash_catalog("a").suggestions()),
        ["mine", "ours", "compact"]
    );
    assert!(state.slash_catalog("gone").suggestions().len() == 1);
}

#[test]
fn a_new_agent_pane_asks_for_its_commands_and_its_workspaces_skills_once() {
    let mut state = state_with_agent("w");
    let arrived = |state: &mut State, id: &str| {
        state.apply(event(
            EventName::PaneCreated,
            json!({"pane": pane_json(id, "w", "agent")}),
        ))
    };
    // The snapshot's own pane asked for both.
    state.apply(Event::Skills {
        workspace_id: Some("w".into()),
        listing: skill_listing(json!([])),
    });
    let effects = arrived(&mut state, "b");
    assert!(effects.contains(&Effect::LoadCommands("b".into())));
    assert!(!effects.contains(&Effect::LoadSkills(Some("w".into()))));
}

#[test]
fn a_snapshot_asks_again_for_every_agents_commands_and_every_workspaces_skills() {
    let mut state = State::default();
    let effects = state.apply(Event::Connected(Snapshot {
        workspaces: vec![workspace("w"), workspace("v")],
        panes: vec![
            agent_pane("a", "w", json!({})),
            agent_pane("b", "w", json!({})),
            agent_pane("c", "v", json!({})),
        ],
        ..Snapshot::default()
    }));
    for wanted in [
        Effect::LoadCommands("a".into()),
        Effect::LoadCommands("b".into()),
        Effect::LoadCommands("c".into()),
        Effect::LoadSkills(Some("w".into())),
        Effect::LoadSkills(Some("v".into())),
    ] {
        assert_eq!(
            effects.iter().filter(|e| **e == wanted).count(),
            1,
            "{wanted:?}"
        );
    }
}

#[test]
fn changed_skills_and_commands_are_listed_again_only_where_they_were_listed() {
    let mut state = state_with_agent("w");
    let notify = |state: &mut State, name, params| state.apply(event(name, params));
    assert_eq!(
        notify(
            &mut state,
            EventName::SkillsChanged,
            json!({"scope": "user"})
        ),
        []
    );
    assert_eq!(
        notify(
            &mut state,
            EventName::PaneCommandsChanged,
            json!({"paneId": "a"})
        ),
        []
    );
    state.apply(Event::PaneCommands {
        pane_id: "a".into(),
        items: Vec::new(),
    });
    for key in [None, Some("w".to_owned()), Some("v".to_owned())] {
        state.apply(Event::Skills {
            workspace_id: key,
            listing: skill_listing(json!([])),
        });
    }
    // The workspace "v" is not one the window holds: its listing was not kept.
    assert_eq!(
        notify(
            &mut state,
            EventName::SkillsChanged,
            json!({"scope": "user"})
        ),
        [
            Effect::LoadSkills(None),
            Effect::LoadSkills(Some("w".into()))
        ]
    );
    assert_eq!(
        notify(
            &mut state,
            EventName::SkillsChanged,
            json!({"scope": "workspace", "workspaceId": "w"})
        ),
        [Effect::LoadSkills(Some("w".into()))]
    );
    assert_eq!(
        notify(
            &mut state,
            EventName::PaneCommandsChanged,
            json!({"paneId": "a"})
        ),
        [Effect::LoadCommands("a".into())]
    );
}

#[test]
fn a_closed_pane_and_a_closed_workspace_leave_nothing_listed() {
    let mut state = state_with_agent("w");
    state.apply(Event::PaneCommands {
        pane_id: "a".into(),
        items: Vec::new(),
    });
    state.apply(Event::Skills {
        workspace_id: Some("w".into()),
        listing: skill_listing(json!([])),
    });
    state.apply(event(
        EventName::PaneClosed,
        json!({"paneId": "a", "workspaceId": "w"}),
    ));
    state.apply(event(
        EventName::WorkspaceClosed,
        json!({"workspaceId": "w"}),
    ));
    assert!(state.commands.is_empty());
    assert!(state.skills.is_empty());
}
