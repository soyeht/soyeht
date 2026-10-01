//! End-to-end engine tests for the project panel and editor tabs.

use super::test_fixture::EditorFixture;
use serde_json::{Value, json};
use std::fs;
use uuid::Uuid;

fn key() -> String {
    Uuid::new_v4().to_string()
}

fn tabs(fixture: &mut EditorFixture) -> Vec<(String, bool)> {
    let workspace_id = fixture.workspace_id.clone();
    fixture.call("editor.tabs.list", json!({"workspaceId":workspace_id}))["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tab| {
            (
                tab["path"].as_str().unwrap().to_owned(),
                tab["preview"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn pane_of(opened: &Value) -> String {
    opened["pane"]["paneId"].as_str().unwrap().to_owned()
}

fn file_of(opened: &Value) -> String {
    opened["file"]["fileId"].as_str().unwrap().to_owned()
}

fn close(fixture: &mut EditorFixture, pane_id: &str) {
    fixture.call(
        "pane.close",
        json!({"paneId":pane_id,"idempotencyKey":key()}),
    );
}

fn tab(path: &str, preview: bool) -> (String, bool) {
    (path.to_owned(), preview)
}

#[tokio::test]
async fn preview_tabs_replace_each_other_until_edited_and_tabs_keep_their_order() {
    let files = [
        ("a.rs", "a\n"),
        ("b.rs", "b\n"),
        ("c.rs", "c\n"),
        ("d.rs", "d\n"),
    ];
    let mut fixture = EditorFixture::new(&files).await;
    fixture.call("events.subscribe", json!({"since":"0"}));
    let a = fixture.open("a.rs", false);
    let b = fixture.open("b.rs", true);
    assert_eq!(
        tabs(&mut fixture),
        vec![tab("a.rs", false), tab("b.rs", true)]
    );

    let c = fixture.open("c.rs", true);
    assert_eq!(
        tabs(&mut fixture),
        vec![tab("a.rs", false), tab("c.rs", true)]
    );
    assert!(fixture.core.panes[&pane_of(&b)].stored.closed);
    let events = fixture.notifications().await;
    assert!(events.iter().any(|event| event["method"] == "pane.closed"
        && event["params"]["paneId"] == pane_of(&b).as_str()));
    assert!(
        events
            .iter()
            .any(|event| event["method"] == "editor.tabs_updated")
    );

    // Editing a preview makes it permanent, so the next preview opens beside it.
    fixture.type_at(&file_of(&c), 0, 1, "!");
    assert_eq!(
        tabs(&mut fixture),
        vec![tab("a.rs", false), tab("c.rs", false)]
    );
    let d = fixture.open("d.rs", true);
    assert_eq!(
        tabs(&mut fixture),
        vec![tab("a.rs", false), tab("c.rs", false), tab("d.rs", true)]
    );
    // Opening it for keeps (double click) pins it in place.
    assert_eq!(pane_of(&fixture.open("d.rs", false)), pane_of(&d));
    assert_eq!(
        tabs(&mut fixture),
        vec![tab("a.rs", false), tab("c.rs", false), tab("d.rs", false)]
    );

    let workspace_id = fixture.workspace_id.clone();
    let moved = fixture.call(
        "editor.tabs.move",
        json!({"workspaceId":workspace_id,"paneId":pane_of(&d),"index":0,"idempotencyKey":key()}),
    );
    assert_eq!(moved["tabs"][0]["path"], "d.rs");
    close(&mut fixture, &pane_of(&a));
    assert_eq!(
        tabs(&mut fixture),
        vec![tab("d.rs", false), tab("c.rs", false)]
    );
}

#[tokio::test]
async fn closing_a_dirty_tab_saves_it_and_a_held_draft_survives_reopening() {
    let mut fixture = EditorFixture::new(&[("main.rs", "fn main() {}\n")]).await;
    let opened = fixture.open("main.rs", false);
    fixture.type_at(&file_of(&opened), 0, 0, "// saved on close\n");
    close(&mut fixture, &pane_of(&opened));
    assert_eq!(
        fs::read_to_string(fixture.project().join("main.rs")).unwrap(),
        "// saved on close\nfn main() {}\n"
    );

    // With autosave held, the draft stays in the database and comes back.
    fs::write(fixture.root.join("engine/test-pause-editor-autosave"), "").unwrap();
    let reopened = fixture.open("main.rs", false);
    assert_eq!(reopened["file"]["dirty"], false);
    fixture.type_at(&file_of(&reopened), 0, 0, "x");
    close(&mut fixture, &pane_of(&reopened));
    let again = fixture.open("main.rs", false);
    assert_eq!(again["file"]["dirty"], true);
    assert!(fixture.text(&file_of(&again)).starts_with("x// saved"));
    fs::remove_file(fixture.root.join("engine/test-pause-editor-autosave")).unwrap();
    fixture.at(10_000);
    fixture.core.advance_editor().unwrap();
    assert!(
        fs::read_to_string(fixture.project().join("main.rs"))
            .unwrap()
            .starts_with("x// saved")
    );
}

#[tokio::test]
async fn rename_moves_open_buffers_drafts_and_tabs_with_the_files() {
    let mut fixture = EditorFixture::new(&[("src/a.rs", "a\n"), ("src/b.rs", "b\n")]).await;
    let workspace_id = fixture.workspace_id.clone();
    let opened = fixture.open("src/a.rs", false);
    fs::write(fixture.root.join("engine/test-pause-editor-autosave"), "").unwrap();
    fixture.type_at(&file_of(&opened), 0, 0, "draft ");

    let renamed = fixture.call(
        "project.entry.rename",
        json!({"workspaceId":workspace_id,"from":"src","to":"lib","idempotencyKey":key()}),
    );
    assert_eq!(renamed["files"][0]["path"], "lib/a.rs");
    assert_eq!(renamed["files"][0]["dirty"], true);
    assert!(fixture.project().join("lib/b.rs").is_file());
    assert!(!fixture.project().join("src").exists());
    assert_eq!(tabs(&mut fixture), vec![tab("lib/a.rs", false)]);
    assert_eq!(fixture.read(&file_of(&opened))["file"]["path"], "lib/a.rs");

    fs::remove_file(fixture.root.join("engine/test-pause-editor-autosave")).unwrap();
    fixture.at(10_000);
    fixture.core.advance_editor().unwrap();
    assert_eq!(
        fs::read_to_string(fixture.project().join("lib/a.rs")).unwrap(),
        "draft a\n"
    );

    let file = fixture.call(
        "project.entry.rename",
        json!({"workspaceId":workspace_id,"from":"lib/a.rs","to":"lib/a.md","idempotencyKey":key()}),
    );
    assert_eq!(file["files"][0]["language"], "markdown");
    let pane = &fixture.core.panes[&pane_of(&opened)].stored.pane;
    assert_eq!(pane.name, "a.md");

    let collision = fixture.try_call(
        "project.entry.rename",
        json!({"workspaceId":workspace_id,"from":"lib/b.rs","to":"lib/a.md","idempotencyKey":key()}),
    );
    assert!(collision.is_err());
    assert_eq!(
        fs::read_to_string(fixture.project().join("lib/b.rs")).unwrap(),
        "b\n"
    );
}

#[tokio::test]
async fn create_and_delete_stay_in_the_workspace_and_close_deleted_tabs() {
    let mut fixture = EditorFixture::new(&[("notes/todo.md", "todo\n")]).await;
    let workspace_id = fixture.workspace_id.clone();
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.project().join("escape")).unwrap();

    let created = fixture.call(
        "project.entry.create",
        json!({"workspaceId":workspace_id,"path":"src/main.rs","kind":"file","idempotencyKey":key()}),
    );
    assert_eq!(created["entry"]["path"], "src/main.rs");
    assert!(fixture.project().join("src/main.rs").is_file());
    for (method, params) in [
        (
            "project.entry.create",
            json!({"workspaceId":workspace_id,"path":"../planted","kind":"file","idempotencyKey":key()}),
        ),
        (
            "project.entry.create",
            json!({"workspaceId":workspace_id,"path":"escape/planted","kind":"file","idempotencyKey":key()}),
        ),
        (
            "project.entry.rename",
            json!({"workspaceId":workspace_id,"from":"notes/todo.md","to":"escape/todo.md","idempotencyKey":key()}),
        ),
        (
            "project.entry.delete",
            json!({"workspaceId":workspace_id,"path":"../outside","permanent":true,"idempotencyKey":key()}),
        ),
        (
            "project.tree.list",
            json!({"workspaceId":workspace_id,"path":"escape"}),
        ),
    ] {
        assert!(fixture.try_call(method, params).is_err(), "{method}");
    }
    assert!(!outside.join("planted").exists());
    assert!(fixture.project().join("notes/todo.md").is_file());

    let opened = fixture.open("notes/todo.md", false);
    fixture.call(
        "project.entry.delete",
        json!({"workspaceId":workspace_id,"path":"notes","permanent":true,"idempotencyKey":key()}),
    );
    assert!(!fixture.project().join("notes").exists());
    assert!(fixture.core.panes[&pane_of(&opened)].stored.closed);
    assert!(tabs(&mut fixture).is_empty());
}

#[tokio::test]
async fn tree_lists_lazily_and_tells_the_watcher_when_a_folder_changes() {
    let mut fixture = EditorFixture::new(&[
        (".gitignore", "build/\n"),
        ("src/lib.rs", "lib\n"),
        ("build/out.o", "binary\n"),
    ])
    .await;
    fs::create_dir_all(fixture.project().join(".git")).unwrap();
    let workspace_id = fixture.workspace_id.clone();
    let root = fixture.call(
        "project.tree.list",
        json!({"workspaceId":workspace_id,"path":""}),
    );
    let names = |listing: &Value| -> Vec<String> {
        listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(&root), vec!["src", ".gitignore"]);
    let all = fixture.call(
        "project.tree.list",
        json!({"workspaceId":workspace_id,"path":"","showIgnored":true}),
    );
    assert_eq!(names(&all), vec!["build", "src", ".gitignore"]);
    assert_eq!(all["entries"][0]["ignored"], true);

    fixture.call(
        "project.tree.watch",
        json!({"workspaceId":workspace_id,"paths":["","src"]}),
    );
    fixture.notifications().await;
    fs::write(fixture.project().join("src/new.rs"), "").unwrap();
    fixture.at(5_000);
    fixture.core.advance_project_tree().unwrap();
    let changed = fixture.notifications().await;
    assert_eq!(
        changed,
        vec![json!({"jsonrpc":"2.0","method":"project.tree_changed",
            "params":{"workspaceId":workspace_id,"paths":["src"]}})]
    );
    fixture.at(9_000);
    fixture.core.advance_project_tree().unwrap();
    assert!(fixture.notifications().await.is_empty());
}
