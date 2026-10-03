//! Soyeht skills through the engine API with the fake ACP agent (ADR 0075).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::{Value, json};
use std::{fs, path::PathBuf, thread, time::Duration, time::Instant};
use support::{ASYNC_ASSERTION_TIMEOUT, Client, Rig};

fn subscribe(client: &mut Client) {
    let (snapshot, _) = client.snapshot();
    client.call("events.subscribe", json!({"since":snapshot["cursor"]}));
    client.messages.clear();
}

fn names(listing: &Value) -> Vec<String> {
    listing["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_owned())
        .collect()
}

fn item<'a>(listing: &'a Value, name: &str) -> &'a Value {
    listing["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("{name} is not listed: {listing}"))
}

fn user_store(rig: &Rig) -> PathBuf {
    rig.path.join("skills/.agents/skills")
}

#[test]
fn skills_are_created_edited_validated_watched_and_deleted() {
    let rig = Rig::new();
    let mut client = rig.client();
    subscribe(&mut client);
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Skills","path":rig.path,"idempotencyKey":"skills-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();

    let user = client.call("skills.list", json!({}));
    assert_eq!(names(&user), ["create-skill"]);
    assert_eq!(
        user["userDirectory"],
        json!(user_store(&rig).display().to_string())
    );
    let guide = client.call("skills.read", json!({"name":"create-skill"}));
    assert!(
        guide["content"]
            .as_str()
            .unwrap()
            .contains("zero-context agent")
    );

    // Listing a workspace follows its skill folder, which does not exist yet.
    let listed = client.call("skills.list", json!({"workspaceId":workspace}));
    assert_eq!(names(&listed), ["create-skill"]);
    let create = json!({"name":"review","description":"Reviews a change.",
        "instructions":"Read the diff.","workspaceId":workspace,"idempotencyKey":"create-review"});
    let created = client.call("skills.create", create.clone());
    assert_eq!(created, client.call("skills.create", create));
    assert_eq!(created["skill"]["scope"], "workspace");
    assert_eq!(
        created["skill"]["path"],
        json!(rig.path.join(".agents/skills/review").display().to_string())
    );
    client.wait_for_notification("skills.changed", |event| event["workspaceId"] == workspace);

    let listed = client.call("skills.list", json!({"workspaceId":workspace}));
    assert_eq!(names(&listed), ["create-skill", "review"]);

    let read = client.call(
        "skills.read",
        json!({"name":"review","workspaceId":workspace}),
    );
    let edited = read["content"]
        .as_str()
        .unwrap()
        .replace("Read the diff.", "Read the whole diff.");
    let stale = client.request(
        "skills.update",
        json!({"name":"review","workspaceId":workspace,"content":edited,
            "baseRevision":"stale","idempotencyKey":"stale-review"}),
    );
    assert_eq!(stale["error"]["data"]["kind"], "stale_version");
    let renamed = client.request(
        "skills.update",
        json!({"name":"review","workspaceId":workspace,
            "content":edited.replace("name: review","name: other"),
            "baseRevision":read["skill"]["revision"],"idempotencyKey":"renamed-review"}),
    );
    assert_eq!(renamed["error"]["data"]["kind"], "invalid_params");
    assert_eq!(
        renamed["error"]["message"],
        "The name must match the folder name."
    );
    let updated = client.call(
        "skills.update",
        json!({"name":"review","workspaceId":workspace,"content":edited,
            "baseRevision":read["skill"]["revision"],"idempotencyKey":"update-review"}),
    );
    assert_ne!(updated["skill"]["revision"], read["skill"]["revision"]);

    // A folder written by hand or by an agent is listed with its problem.
    client.messages.clear();
    let broken = user_store(&rig).join("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(
        broken.join("SKILL.md"),
        "---\nname: broken\n---\nNo description.\n",
    )
    .unwrap();
    client.wait_for_notification("skills.changed", |event| event["scope"] == "user");
    let listed = client.call("skills.list", json!({"workspaceId":workspace}));
    assert_eq!(
        item(&listed, "broken")["problem"],
        "The frontmatter has no description."
    );
    assert!(item(&listed, "broken").get("description").is_none());

    client.messages.clear();
    let deleted = client.call(
        "skills.delete",
        json!({"name":"review","workspaceId":workspace,"idempotencyKey":"delete-review"}),
    );
    assert_eq!(deleted, json!({"name":"review","removed":true}));
    client.wait_for_notification("skills.changed", |event| event["workspaceId"] == workspace);
    let missing = client.request(
        "skills.read",
        json!({"name":"review","workspaceId":workspace}),
    );
    assert_eq!(missing["error"]["data"]["kind"], "not_found");
}

fn commands(client: &mut Client, pane: &str) -> Vec<String> {
    client.call("pane.commands.list", json!({"paneId":pane}))["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["name"].as_str().unwrap().to_owned())
        .collect()
}

fn wait_for_reply(client: &mut Client, pane: &str, text: &str) {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let items = client.call("pane.transcript", json!({"paneId":pane,"limit":100}))["items"]
            .as_array()
            .unwrap()
            .clone();
        if items
            .iter()
            .any(|item| item["role"] == "assistant" && item["text"] == text)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "no reply {text}: {}",
            json!(items)
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_agent_loads_invokes_and_writes_skills_and_its_commands_reach_clients() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    subscribe(&mut client);

    // The session received the skills home; its `.claude/skills` link shows
    // the seeded guide to the agent, which advertises it as a command.
    let session_id = client.pane(&pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let native = rig.native_session(&session_id);
    assert_eq!(
        native["additional_directories"],
        json!([rig.path.join("skills").display().to_string()])
    );
    assert_eq!(
        commands(&mut client, &pane),
        ["fake-status", "create-skill"]
    );

    // An agent asked to create a skill writes the folder with its file tools.
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"write-skill notes","idempotencyKey":"write-skill"}),
    );
    wait_for_reply(&mut client, &pane, "Wrote skill notes");
    client.wait_for_notification("pane.commands_changed", |event| event["paneId"] == pane);
    assert_eq!(
        commands(&mut client, &pane),
        ["fake-status", "create-skill", "notes"]
    );
    client.wait_for_notification("skills.changed", |event| event["scope"] == "user");
    let listed = client.call("skills.list", json!({}));
    assert_eq!(
        item(&listed, "notes")["description"],
        "Written by the test agent."
    );

    // A client invokes a skill as the ACP prompt `/name arguments`.
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"/notes tomorrow","idempotencyKey":"invoke-skill"}),
    );
    wait_for_reply(
        &mut client,
        &pane,
        "Skill notes: Say notes done. Arguments: tomorrow",
    );
}
