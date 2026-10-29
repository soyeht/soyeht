use super::*;

#[test]
fn workspace_pane_layout_is_validated_persisted_and_restored() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Layout", &rig.path, "layout-workspace");
    let first_pane = client.call(
        "pane.create",
        json!({
            "workspaceId":workspace,"mode":"terminal","name":"First",
            "idempotencyKey":"layout-pane-first"
        }),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_pane = client.call(
        "pane.create",
        json!({
            "workspaceId":workspace,"mode":"terminal","name":"Second",
            "idempotencyKey":"layout-pane-second"
        }),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let layout = json!({
        "version":2,"mode":"custom",
        "tree":{"axis":"horizontal","children":[
            {"weight":2.0,"node":{"paneId":first_pane,"kind":"terminal"}},
            {"weight":1.0,"node":{"paneId":second_pane,"kind":"terminal"}}
        ]}
    });
    let saved = client.call(
        "workspace.layout.set",
        json!({
            "workspaceId":workspace,"layout":layout,
            "idempotencyKey":"save-layout"
        }),
    );
    assert_eq!(saved["workspace"]["layout"], layout);

    let missing = client.request(
        "workspace.layout.set",
        json!({
            "workspaceId":workspace,
            "layout":{"version":2,"mode":"custom",
                "tree":{"paneId":"missing","kind":"terminal"}},
            "idempotencyKey":"missing-layout-pane"
        }),
    );
    assert_eq!(missing["error"]["data"]["kind"], "invalid_params");
    let unknown = client.request(
        "workspace.layout.set",
        json!({
            "workspaceId":workspace,
            "layout":{"version":99,"mode":"custom","tree":null},
            "idempotencyKey":"unknown-layout-version"
        }),
    );
    assert_eq!(unknown["error"]["data"]["kind"], "invalid_params");

    client.call(
        "pane.close",
        json!({"paneId":first_pane,"idempotencyKey":"close-first-layout-pane"}),
    );
    let pruned_layout = json!({
        "version":2,"mode":"custom",
        "tree":{"paneId":second_pane,"kind":"terminal"}
    });

    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let workspaces = restored.call("workspace.list", json!({}));
    let restored_layout = &workspaces["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["workspaceId"] == workspace)
        .unwrap()["layout"];
    assert_eq!(restored_layout, &pruned_layout);
}

#[test]
fn writing_section_listing_ignores_protected_workspace_siblings() {
    let rig = Rig::new();
    fs::create_dir_all(rig.path.join("Writing/Texts/Research")).unwrap();
    fs::write(
        rig.path.join("Writing/Texts/Research/notes.md"),
        "# Notes\n",
    )
    .unwrap();
    let protected = rig.path.join("Pictures/Photos Library.photoslibrary");
    fs::create_dir_all(&protected).unwrap();
    fs::write(protected.join("private.md"), "private\n").unwrap();
    deny_everyone(&protected).unwrap();

    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Scoped writing", &rig.path, "scoped-writing");
    let listed = client.request(
        "writing.section.list",
        json!({"workspaceId":workspace,"section":"texts","limit":50}),
    );
    allow_owner(&protected).unwrap();

    assert!(listed.get("error").is_none(), "{listed}");
    assert_eq!(listed["result"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        listed["result"]["entries"][0]["path"],
        "Writing/Texts/Research/notes.md"
    );
    assert!(client.call("engine.status", json!({}))["version"].is_string());
}

#[test]
fn pane_created_events_carry_replica_metadata_for_other_clients() {
    let rig = Rig::new();
    fs::create_dir_all(rig.path.join("Writing/Texts")).unwrap();
    fs::write(rig.path.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        rig.path.join("Writing/Texts/notes.md"),
        "# Notes\n\nShared live.\n",
    )
    .unwrap();

    let mut writer = rig.client();
    let workspace = create_workspace(
        &mut writer,
        "Live replica",
        &rig.path,
        "live-replica-workspace",
    );
    let mut observer = rig.client();
    let (snapshot, _) = observer.snapshot();
    observer.call("events.subscribe", json!({"since":snapshot["cursor"]}));
    observer.messages.clear();

    writer.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"editor","path":"main.rs","idempotencyKey":"live-editor"}),
    );
    writer.call(
        "pane.create",
        json!({
            "workspaceId":workspace,
            "mode":"writing",
            "path":"Writing/Texts/notes.md",
            "writingKind":"text",
            "writingSection":"texts",
            "idempotencyKey":"live-writing"
        }),
    );
    observer.call("engine.status", json!({}));

    let events = observer.notifications("pane.created");
    let editor = events
        .iter()
        .find(|event| event["pane"]["mode"] == "editor")
        .expect("editor pane event");
    assert_eq!(editor["file"]["paneId"], editor["pane"]["paneId"]);
    assert_eq!(editor["file"]["path"], "main.rs");
    assert!(editor.get("writingDocument").is_none());
    assert!(editor.get("viewport").is_none());

    let writing = events
        .iter()
        .find(|event| event["pane"]["mode"] == "writing")
        .expect("writing pane event");
    assert_eq!(writing["file"]["paneId"], writing["pane"]["paneId"]);
    assert_eq!(writing["writingDocument"]["file"], writing["file"]);
    assert_eq!(writing["writingDocument"]["kind"], "text");
    assert!(writing.get("viewport").is_none());
}

#[test]
fn writing_documents_reuse_owned_files_and_restore_preferences_and_structure() {
    let mut rig = Rig::new();
    fs::create_dir_all(rig.path.join("Writing/Texts")).unwrap();
    fs::create_dir_all(rig.path.join("Writing/Presentations")).unwrap();
    fs::write(
        rig.path.join("Writing/Texts/field-notes.md"),
        "# Field notes\n\nA bounded writing document.\n",
    )
    .unwrap();
    fs::write(
        rig.path.join("Writing/Presentations/review.md"),
        "# First\nSpeaker note\n---\n# Second\nAnother note\n",
    )
    .unwrap();
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Writing", &rig.path, "writing-workspace");

    let opened = client.call(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":"Writing/Texts/field-notes.md","kind":"text","idempotencyKey":"open-text"}),
    );
    let file_id = opened["document"]["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let pane_id = opened["pane"]["paneId"].as_str().unwrap().to_owned();
    assert_eq!(opened["pane"]["mode"], "writing");
    assert_eq!(opened["pane"]["writingKind"], "text");
    assert_eq!(opened["pane"]["writingSection"], "texts");
    assert_eq!(opened["document"]["file"]["paneId"], pane_id);
    let repeated = client.call(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":"Writing/Texts/field-notes.md","kind":"text","idempotencyKey":"open-text"}),
    );
    assert_eq!(repeated, opened);
    let pane_route = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"writing","path":"Writing/Texts/field-notes.md",
        "writingKind":"text","writingSection":"texts","idempotencyKey":"open-writing-pane"}),
    );
    assert_eq!(pane_route["paneId"], pane_id);

    let preferred = client.call(
        "writing.preferences.set",
        json!({"fileId":file_id,"baseRevision":"1","font":"sans","focusMode":true,"fullWidth":true,"idempotencyKey":"writing-preferences"}),
    );
    assert_eq!(preferred["document"]["font"], "sans");
    assert_eq!(preferred["document"]["focusMode"], true);
    assert_eq!(preferred["document"]["fullWidth"], true);
    let listed = client.call(
        "writing.section.list",
        json!({"workspaceId":workspace,"section":"texts","limit":50}),
    );
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    assert_eq!(listed["entries"][0]["fileId"], file_id);

    let slides = client.call(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":"Writing/Presentations/review.md","kind":"presentation","idempotencyKey":"open-presentation"}),
    );
    let slides_file = slides["document"]["file"]["fileId"].clone();
    let screenplay = client.call(
        "writing.document.create",
        json!({
            "workspaceId":workspace,"section":"screenplays","title":"No Full Width",
            "idempotencyKey":"create-no-full-width-screenplay"
        }),
    );
    let screenplay_file = screenplay["document"]["file"]["fileId"].clone();
    for (file_id, key) in [
        (slides_file.clone(), "presentation-full-width"),
        (screenplay_file, "screenplay-full-width"),
    ] {
        let refused = client.request(
            "writing.preferences.set",
            json!({
                "fileId":file_id,"baseRevision":"1","fullWidth":true,
                "idempotencyKey":key
            }),
        );
        assert_eq!(refused["error"]["data"]["kind"], "invalid_params");
        assert_eq!(
            refused["error"]["message"],
            "Full width is available only for text documents"
        );
    }
    let structure = client.call(
        "writing.structure",
        json!({"fileId":slides_file,"revision":"1"}),
    );
    assert_eq!(structure["slides"].as_array().unwrap().len(), 2);
    assert_eq!(structure["slides"][0]["heading"], "First");
    assert_eq!(structure["slides"][1]["notes"], "Another note");
    assert!(structure["scenes"].as_array().unwrap().is_empty());

    let escaped = client.request(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":"Writing/Texts/../Presentations/review.md","kind":"text","idempotencyKey":"escaped-writing"}),
    );
    assert_eq!(escaped["error"]["data"]["kind"], "writing_section_refused");
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let writing = restored.section("writing");
    let restored_document = writing
        .iter()
        .find(|document| document["file"]["fileId"] == file_id)
        .unwrap();
    assert_eq!(restored_document["font"], "sans");
    assert_eq!(restored_document["focusMode"], true);
    assert_eq!(restored_document["fullWidth"], true);
    assert_eq!(restored_document["file"]["paneId"], pane_id);
}
