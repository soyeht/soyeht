//! Opening a writing path that does not exist yet creates the document (issue 347).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::{Value, json};
use std::fs;
use support::{Client, Rig};

fn workspace(client: &mut Client, rig: &Rig) -> String {
    client.call(
        "workspace.create",
        json!({"name":"New writing","path":rig.path,"idempotencyKey":"new-writing-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn create_text_pane(client: &mut Client, workspace: &str, path: &str, key: &str) -> Value {
    client.request(
        "pane.create",
        json!({
            "workspaceId":workspace,
            "mode":"writing",
            "path":path,
            "writingKind":"text",
            "writingSection":"texts",
            "idempotencyKey":key
        }),
    )
}

fn open_text_document(client: &mut Client, workspace: &str, path: &str, key: &str) -> Value {
    client.request(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":path,"kind":"text","idempotencyKey":key}),
    )
}

fn restart(rig: &mut Rig, client: Client) -> Client {
    drop(client);
    rig.kill_engine();
    rig.start();
    rig.client()
}

fn open_paths(client: &mut Client) -> Vec<String> {
    client
        .section("writing")
        .iter()
        .map(|document| document["file"]["path"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_new_path_in_an_existing_folder_is_created_and_survives_a_restart() {
    let mut rig = Rig::new();
    fs::create_dir_all(rig.path.join("Writing/Texts")).unwrap();
    let mut client = rig.client();
    let workspace = workspace(&mut client, &rig);

    let created = create_text_pane(
        &mut client,
        &workspace,
        "Writing/Texts/varredura.md",
        "new-in-existing-folder",
    );

    assert!(created.get("error").is_none(), "{created}");
    assert_eq!(
        fs::read_to_string(rig.path.join("Writing/Texts/varredura.md")).unwrap(),
        ""
    );
    assert_eq!(open_paths(&mut client), ["Writing/Texts/varredura.md"]);
    let mut restored = restart(&mut rig, client);
    assert_eq!(open_paths(&mut restored), ["Writing/Texts/varredura.md"]);
}

#[test]
fn a_new_path_whose_folders_do_not_exist_creates_them() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let workspace = workspace(&mut client, &rig);

    let path = "Writing/Texts/Research/September/notes.md";
    let opened = open_text_document(&mut client, &workspace, path, "new-with-folders");

    assert!(opened.get("error").is_none(), "{opened}");
    assert_eq!(fs::read_to_string(rig.path.join(path)).unwrap(), "");
    let repeated = open_text_document(&mut client, &workspace, path, "new-with-folders");
    assert_eq!(repeated["result"], opened["result"]);
    let mut restored = restart(&mut rig, client);
    assert_eq!(open_paths(&mut restored), [path]);
}

#[test]
fn a_path_that_cannot_be_created_returns_a_typed_error_instead_of_internal() {
    let rig = Rig::new();
    let mut client = rig.client();
    let workspace = workspace(&mut client, &rig);

    // A folder of the path is a file.
    fs::write(rig.path.join("Writing"), "not a folder").unwrap();
    let blocked = create_text_pane(
        &mut client,
        &workspace,
        "Writing/Texts/blocked.md",
        "blocked-by-file",
    );
    assert_eq!(blocked["error"]["data"]["kind"], "invalid_params");
    assert_ne!(blocked["error"]["message"], "I/O operation failed");
    fs::remove_file(rig.path.join("Writing")).unwrap();

    // A folder of the path is a link that leaves the workspace.
    let outside = rig.path.join("outside-target");
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(rig.path.join("Writing")).unwrap();
    soyeht_platform::fs::symlink(&outside, rig.path.join("Writing/Texts")).unwrap();
    let escaped = create_text_pane(
        &mut client,
        &workspace,
        "Writing/Texts/escaped.md",
        "escaped-by-link",
    );
    assert_eq!(escaped["error"]["data"]["kind"], "invalid_params");
    assert!(!outside.join("escaped.md").exists());

    // The path is outside the writing section.
    let outside_section = create_text_pane(
        &mut client,
        &workspace,
        "Elsewhere/note.md",
        "outside-section",
    );
    assert_eq!(
        outside_section["error"]["data"]["kind"],
        "writing_section_refused"
    );
    assert!(!rig.path.join("Elsewhere").exists());
}
