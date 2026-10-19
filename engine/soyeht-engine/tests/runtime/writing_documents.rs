use super::*;

#[test]
fn writing_structure_counts_the_whole_document_and_recalculates_after_an_edit() {
    let rig = Rig::new();
    fs::create_dir_all(rig.path.join("Writing/Texts")).unwrap();
    let text = format!("{}{}\n", "\n".repeat(2_100), vec!["word"; 914].join(" "));
    fs::write(rig.path.join("Writing/Texts/ensaio.md"), text).unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Whole document count","path":rig.path,"idempotencyKey":"word-count-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let opened = client.call(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":"Writing/Texts/ensaio.md","kind":"text","idempotencyKey":"open-word-count"}),
    );
    let file_id = opened["document"]["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let initial = client.call(
        "writing.structure",
        json!({"fileId":file_id,"revision":"1"}),
    );
    assert_eq!(initial["wordCount"], 914);

    let edited = client.call(
        "file.write",
        json!({
            "fileId":file_id,"baseRevision":"1","author":local_file_author(),
            "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":0}},"text":"added\n"}],
            "idempotencyKey":"add-counted-word"
        }),
    );
    assert_eq!(edited["file"]["revision"], "2");
    let revised = client.call(
        "writing.structure",
        json!({"fileId":file_id,"revision":"2"}),
    );
    assert_eq!(revised["wordCount"], 915);
}

#[test]
fn writing_document_creation_is_durable_disjoint_and_idempotent_across_restart() {
    let mut rig = Rig::new();
    let repository = rig.path.join("writing-create-repository");
    fs::create_dir(&repository).unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Writing create","path":repository,"idempotencyKey":"writing-create-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();

    let created = client.call(
        "writing.document.create",
        json!({
            "workspaceId":workspace,"section":"texts","title":"Field Notes",
            "relativeFolder":"Research/September","idempotencyKey":"create-field-notes"
        }),
    );
    assert_eq!(created["document"]["kind"], "text");
    assert_eq!(created["document"]["section"], "texts");
    assert_eq!(
        created["document"]["file"]["path"],
        "Writing/Texts/Research/September/field-notes.md"
    );
    assert_eq!(created["pane"]["mode"], "writing");
    assert_eq!(created["viewport"]["fromLine"], 0);
    assert_eq!(created["viewport"]["toLine"], 1);
    assert_eq!(created["viewport"]["lines"][0]["text"], "");
    assert_eq!(
        fs::read_to_string(repository.join("Writing/Texts/Research/September/field-notes.md"))
            .unwrap(),
        ""
    );
    let repeated = client.call(
        "writing.document.create",
        json!({
            "workspaceId":workspace,"section":"texts","title":"Field Notes",
            "relativeFolder":"Research/September","idempotencyKey":"create-field-notes"
        }),
    );
    assert_eq!(repeated, created);
    let duplicate = client.request(
        "writing.document.create",
        json!({
            "workspaceId":workspace,"section":"texts","title":"Field Notes",
            "relativeFolder":"Research/September","idempotencyKey":"create-field-notes-again"
        }),
    );
    assert_eq!(duplicate["error"]["data"]["kind"], "invalid_params");

    let screenplay = client.call(
        "writing.document.create",
        json!({
            "workspaceId":workspace,"section":"screenplays","title":"Night Train",
            "idempotencyKey":"create-night-train"
        }),
    );
    assert_eq!(screenplay["document"]["kind"], "screenplay");
    assert_eq!(
        screenplay["document"]["file"]["path"],
        "Writing/Screenplays/night-train.fountain"
    );
    let texts = client.call(
        "writing.section.list",
        json!({"workspaceId":workspace,"section":"texts","limit":50}),
    );
    let screenplays = client.call(
        "writing.section.list",
        json!({"workspaceId":workspace,"section":"screenplays","limit":50}),
    );
    assert_eq!(texts["entries"].as_array().unwrap().len(), 1);
    assert_eq!(screenplays["entries"].as_array().unwrap().len(), 1);

    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let after_restart = restored.call(
        "writing.document.create",
        json!({
            "workspaceId":workspace,"section":"texts","title":"Field Notes",
            "relativeFolder":"Research/September","idempotencyKey":"create-field-notes"
        }),
    );
    assert_eq!(after_restart, created);
    let writing = restored.section("writing");
    assert_eq!(writing.len(), 2);
}

#[test]
fn writing_format_uses_the_owned_revisioned_file_and_survives_restart() {
    let mut rig = Rig::new();
    let repository = rig.path.join("writing-format-repository");
    fs::create_dir_all(repository.join("Writing/Texts")).unwrap();
    let path = repository.join("Writing/Texts/field-notes.md");
    fs::write(&path, "# Field notes\n\nDraft safely.\n").unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Writing format","path":repository,"idempotencyKey":"writing-format-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let opened = client.call(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":"Writing/Texts/field-notes.md","kind":"text","idempotencyKey":"writing-format-open"}),
    );
    let file_id = opened["document"]["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let params = json!({
        "fileId":file_id,"baseRevision":"1","range":{"start":{"line":2,"column":0},"end":{"line":2,"column":5}},
        "kind":"bold","idempotencyKey":"writing-format-bold"
    });
    let formatted = client.call("writing.format", params.clone());
    assert_eq!(formatted["file"]["revision"], "2");
    assert_eq!(client.call("writing.format", params), formatted);
    let read = client.call(
        "file.read",
        json!({"fileId":file_id,"fromLine":0,"lineCount":10}),
    );
    assert_eq!(read["lines"][2]["text"], "**Draft** safely.");
    let stale = client.request(
        "writing.format",
        json!({
            "fileId":file_id,"baseRevision":"1","range":{"start":{"line":2,"column":0},"end":{"line":2,"column":5}},
            "kind":"italic","idempotencyKey":"writing-format-stale"
        }),
    );
    assert_eq!(stale["error"]["data"]["kind"], "stale_version");

    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while fs::read_to_string(&path).unwrap() != "# Field notes\n\n**Draft** safely.\n" {
        assert!(
            Instant::now() < deadline,
            "formatted draft did not autosave"
        );
        thread::sleep(Duration::from_millis(20));
    }
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let repeated = restored.call(
        "writing.format",
        json!({
            "fileId":file_id,"baseRevision":"1","range":{"start":{"line":2,"column":0},"end":{"line":2,"column":5}},
            "kind":"bold","idempotencyKey":"writing-format-bold"
        }),
    );
    assert_eq!(repeated["file"]["revision"], "2");
    assert_eq!(restored.section("writing")[0]["file"]["revision"], "2");
}

#[test]
fn screenplay_variants_are_agent_authored_writer_selected_and_restart_durable() {
    let mut rig = Rig::new();
    let (mut human, workspace, file_id, path) = screenplay_variant_fixture(&rig);
    let (mut agent_client, pane) = rig.agent_in_workspace(&workspace);
    let agent_name = agent_client.pane(&pane)["name"]
        .as_str()
        .unwrap()
        .to_owned();
    let request = json!({"fileId":file_id,"lineId":"element-3","baseRevision":"1",
        "agentPaneId":pane,"instruction":"Write a quieter alternate.",
        "idempotencyKey":"request-screenplay-variant"});
    let requested = human.call("writing.variant.request", request.clone());
    assert_eq!(requested["delivered"], true);
    assert_eq!(human.call("writing.variant.request", request), requested);

    let binding = rig.database_probe().agent_binding(&pane).unwrap();
    let (mut agent, welcome) = binding_hello(&rig.path.join("engine.sock"), &binding);
    assert_eq!(welcome["result"]["role"], "agent");
    let context = binding_request(
        &mut agent,
        2,
        "writing.structure",
        json!({"fileId":file_id,"revision":"1"}),
    );
    assert!(context["result"]["variants"].as_array().unwrap().is_empty());
    let add_params = json!({"fileId":file_id,"lineId":"element-3","baseRevision":"1",
        "text":"Is it late already?","idempotencyKey":"add-screenplay-variant"});
    let added = binding_request(&mut agent, 3, "writing.variant.add", add_params.clone());
    assert!(added.get("error").is_none(), "{added}");
    assert_eq!(
        added["result"]["variants"]["alternates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        added["result"]["variants"]["alternates"][1]["author"]["actor"]["paneId"],
        pane
    );
    assert_eq!(
        added["result"]["variants"]["alternates"][1]["author"]["name"],
        agent_name
    );
    assert_eq!(
        binding_request(&mut agent, 4, "writing.variant.add", add_params)["result"],
        added["result"]
    );
    let denied = binding_request(
        &mut agent,
        5,
        "writing.variant.select",
        json!({"fileId":file_id,"lineId":"element-3",
            "variantId":added["result"]["variants"]["alternates"][1]["variantId"],
            "baseRevision":"1","idempotencyKey":"agent-cannot-select"}),
    );
    assert_eq!(denied["error"]["data"]["kind"], "forbidden_role");

    let alternate_id = added["result"]["variants"]["alternates"][1]["variantId"]
        .as_str()
        .unwrap()
        .to_owned();
    let select = json!({"fileId":file_id,"lineId":"element-3","variantId":alternate_id,
        "baseRevision":"1","idempotencyKey":"select-screenplay-variant"});
    let selected = human.call("writing.variant.select", select.clone());
    assert_eq!(selected["document"]["file"]["revision"], "2");
    assert_eq!(selected["variants"]["selectedIndex"], 1);
    assert_eq!(human.call("writing.variant.select", select), selected);
    wait_for_path_text(&path, "INT. STATION - NIGHT\n\nANA\nIs it late already?\n");

    drop(agent);
    drop(agent_client);
    drop(human);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let structure = restored.call(
        "writing.structure",
        json!({"fileId":file_id,"revision":"2"}),
    );
    assert_eq!(structure["variants"][0]["selectedIndex"], 1);
    assert_eq!(
        structure["variants"][0]["alternates"][1]["text"],
        "Is it late already?"
    );

    let remove = json!({"fileId":file_id,"lineId":"element-3","variantId":alternate_id,
        "baseRevision":"2","idempotencyKey":"remove-screenplay-variant"});
    let removed = restored.call("writing.variant.remove", remove.clone());
    assert_eq!(removed["document"]["file"]["revision"], "3");
    assert_eq!(
        removed["variants"]["alternates"].as_array().unwrap().len(),
        1
    );
    assert_eq!(restored.call("writing.variant.remove", remove), removed);
    wait_for_path_text(&path, VARIANT_SCREENPLAY);
    drop(restored);
    rig.kill_engine();
    rig.start();
    let mut final_client = rig.client();
    let final_structure = final_client.call(
        "writing.structure",
        json!({"fileId":file_id,"revision":"3"}),
    );
    assert_eq!(final_structure["variants"][0]["selectedIndex"], 0);
    assert_eq!(
        final_structure["variants"][0]["alternates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(fs::read_to_string(path).unwrap(), VARIANT_SCREENPLAY);
}

const VARIANT_SCREENPLAY: &str = "INT. STATION - NIGHT\n\nANA\nWhat time is it?\n";

fn screenplay_variant_fixture(rig: &Rig) -> (Client, String, String, PathBuf) {
    let repository = rig.path.join("screenplay-variants");
    fs::create_dir_all(repository.join("Writing/Screenplays")).unwrap();
    let path = repository.join("Writing/Screenplays/night-train.fountain");
    fs::write(&path, VARIANT_SCREENPLAY).unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Screenplay variants","path":repository,"idempotencyKey":"variant-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let file_id = client.call(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":"Writing/Screenplays/night-train.fountain",
            "kind":"screenplay","idempotencyKey":"variant-open"}),
    )["document"]["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    (client, workspace, file_id, path)
}

fn wait_for_path_text(path: &std::path::Path, expected: &str) {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while fs::read_to_string(path).unwrap() != expected {
        assert!(Instant::now() < deadline, "writing draft did not autosave");
        thread::sleep(Duration::from_millis(20));
    }
}
