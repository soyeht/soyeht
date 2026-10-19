use super::*;

#[test]
fn language_server_is_scoped_publishes_diagnostics_and_survives_engine_restart() {
    let mut rig = Rig::new();
    let repository = rig.path.join("language-repository");
    fs::create_dir_all(&repository).unwrap();
    fs::write(
        repository.join("main.rs"),
        "// 😀\nfn main() {\n    ready();\n}\n",
    )
    .unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Language","path":repository,"idempotencyKey":"language-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let file_id = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"main.rs","idempotencyKey":"language-open"}),
    )["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();

    let ready = wait_lsp_ready(&mut client, &workspace, 1);
    assert_eq!(ready["language"], "rust");
    let first = language_registry_instance(&rig);
    assert_eq!(first["spawn"]["cwd"], repository.to_str().unwrap());
    assert_eq!(first["spawn"]["argv"], json!(["rust-analyzer"]));
    assert_eq!(first["spawn"]["kind"], "pipes_lsp");
    let instance_id = first["instance_id"].clone();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while rig.database_probe().event_count("lsp.diagnostics").unwrap() == 0 {
        assert!(
            Instant::now() < deadline,
            "diagnostic event was not published"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_language_queries(&mut client, &workspace, &file_id);
    let invalid = client.request(
        "file.definition",
        json!({"fileId":file_id,"revision":"1","position":{"line":0,"column":4}}),
    );
    assert_eq!(invalid["error"]["data"]["kind"], "invalid_params");

    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let status = wait_lsp_ready(&mut restored, &workspace, 1);
    assert_eq!(status["generation"], 1);
    assert_eq!(language_registry_instance(&rig)["instance_id"], instance_id);
    assert_eq!(
        restored.call("file.symbols", json!({"fileId":file_id,"revision":"1"}))["items"][0]["name"],
        "main"
    );
}

#[test]
fn closed_editor_checkpoint_does_not_block_supervisor_reconnect_when_workspace_is_gone() {
    let mut rig = Rig::new();
    let repository = rig.path.join("closed-language-repository");
    fs::create_dir_all(&repository).unwrap();
    fs::write(repository.join("main.rs"), "fn main() {}\n").unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Closed language","path":repository,"idempotencyKey":"closed-language-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let opened = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"main.rs","idempotencyKey":"closed-language-open"}),
    );
    let pane_id = opened["pane"]["paneId"].as_str().unwrap().to_owned();
    wait_lsp_ready(&mut client, &workspace, 1);
    client.call(
        "pane.close",
        json!({"paneId":pane_id,"idempotencyKey":"close-language-pane"}),
    );

    drop(client);
    rig.kill_engine();
    rig.start();
    let mut parked = rig.client();
    assert_eq!(
        parked.call("engine.status", json!({}))["ptyd"]["connected"],
        true
    );
    assert_no_open_language_server(&rig);

    drop(parked);
    rig.kill_engine();
    rig.restart_ptyd();
    fs::remove_dir_all(&repository).unwrap();
    rig.start();

    let mut restored = rig.client();
    let status = restored.call("engine.status", json!({}));
    assert_eq!(status["ptyd"]["connected"], true);
    assert_no_open_language_server(&rig);
}

fn assert_no_open_language_server(rig: &Rig) {
    let registry: Value =
        serde_json::from_slice(&fs::read(rig.path.join("ptyd-registry.json")).unwrap()).unwrap();
    assert!(
        registry["instances"]
            .as_object()
            .unwrap()
            .values()
            .all(|instance| instance["spawn"]["kind"] != "pipes_lsp" || instance["closed"] == true)
    );
}

fn assert_language_queries(client: &mut Client, workspace: &str, file_id: &str) {
    let symbols = client.call("file.symbols", json!({"fileId":file_id,"revision":"1"}));
    assert_eq!(symbols["items"][0]["name"], "main");
    let position = json!({"line":1,"column":3});
    let definition = client.call(
        "file.definition",
        json!({"fileId":file_id,"revision":"1","position":position}),
    );
    assert_eq!(definition["location"]["workspaceId"], workspace);
    assert_eq!(definition["location"]["path"], "main.rs");
    let references = client.call(
        "file.references",
        json!({"fileId":file_id,"revision":"1","position":position}),
    );
    assert_eq!(references["items"].as_array().unwrap().len(), 1);
    assert_eq!(references["items"][0]["path"], "main.rs");
    let diagnostics = client.call("file.diagnostics", json!({"fileId":file_id,"revision":"1"}));
    assert_eq!(diagnostics["items"].as_array().unwrap().len(), 1);
    assert_eq!(diagnostics["items"][0]["message"], "deterministic warning");
}

#[test]
fn language_format_is_revision_checked_durable_and_idempotent() {
    let mut rig = Rig::new();
    let repository = rig.path.join("format-repository");
    fs::create_dir_all(&repository).unwrap();
    fs::write(repository.join("main.rs"), "fn main() {}\n").unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Format","path":repository,"idempotencyKey":"format-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let file_id = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"main.rs","idempotencyKey":"format-open"}),
    )["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_lsp_ready(&mut client, &workspace, 1);
    let params = json!({
        "fileId":file_id,"baseRevision":"1","idempotencyKey":"format-once"
    });
    let formatted = client.call("file.format", params.clone());
    assert_eq!(formatted["file"]["revision"], "2");
    assert_eq!(client.call("file.format", params)["file"]["revision"], "2");
    let read = client.call(
        "file.read",
        json!({"fileId":file_id,"fromLine":0,"lineCount":1}),
    );
    assert_eq!(read["lines"][0]["text"], "// formatted");
    let stale = client.request(
        "file.format",
        json!({"fileId":file_id,"baseRevision":"1","idempotencyKey":"format-stale"}),
    );
    assert_eq!(stale["error"]["data"]["kind"], "stale_version");

    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let repeated = restored.call(
        "file.format",
        json!({"fileId":file_id,"baseRevision":"1","idempotencyKey":"format-once"}),
    );
    assert_eq!(repeated["file"]["revision"], "2");
    let files = restored.section("files");
    assert_eq!(files[0]["revision"], "2");
}

#[test]
fn every_editor_revision_is_synchronized_with_the_live_language_server() {
    let rig = Rig::new();
    let repository = rig.path.join("language-sync-repository");
    fs::create_dir_all(&repository).unwrap();
    let path = repository.join("main.rs");
    fs::write(&path, "fn main() { alpha(); }\n").unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Language sync","path":repository,"idempotencyKey":"sync-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let file_id = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"main.rs","idempotencyKey":"sync-open"}),
    )["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_lsp_ready(&mut client, &workspace, 1);
    wait_for_lsp_revision(&rig, "1");

    client.call(
        "file.write",
        json!({
            "fileId":file_id,"baseRevision":"1","author":local_file_author(),
            "edits":[{"range":{"start":{"line":0,"column":12},"end":{"line":0,"column":17}},"text":"bravo"}],
            "idempotencyKey":"sync-write"
        }),
    );
    wait_for_lsp_revision(&rig, "2");
    wait_for_editor_file(&mut client, &file_id, |file| file["dirty"] == false);
    let version_id =
        client.call("file.history", json!({"fileId":file_id,"limit":1}))["items"][0]["versionId"]
            .clone();

    fs::write(&path, "fn main() { charlie(); }\n").unwrap();
    wait_for_editor_file(&mut client, &file_id, |file| file["revision"] == "3");
    wait_for_lsp_revision(&rig, "3");
    client.call(
        "file.revert",
        json!({
            "fileId":file_id,"versionId":version_id,"baseRevision":"3",
            "idempotencyKey":"sync-revert"
        }),
    );
    wait_for_lsp_revision(&rig, "4");
    wait_for_editor_file(&mut client, &file_id, |file| file["dirty"] == false);

    client.call(
        "file.search",
        json!({
            "workspaceId":workspace,"query":"bravo","replacement":"delta",
            "caseSensitive":true,"limit":10,"idempotencyKey":"sync-replace"
        }),
    );
    wait_for_lsp_revision(&rig, "5");
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "fn main() { delta(); }\n"
    );
}

fn wait_for_lsp_revision(rig: &Rig, revision: &str) {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        if rig
            .database_probe()
            .event_count_by_field("lsp.diagnostics", "revision", revision)
            .unwrap()
            > 0
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "language diagnostics did not reach revision {revision}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_lsp_ready(client: &mut Client, workspace: &str, generation: u64) -> Value {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let result = client.call(
            "lsp.status",
            json!({"workspaceId":workspace,"language":"rust"}),
        );
        let servers = result["servers"].as_array().unwrap();
        if servers.len() == 1
            && servers[0]["state"] == "ready"
            && servers[0]["generation"] == generation
        {
            return servers[0].clone();
        }
        assert!(
            Instant::now() < deadline,
            "language server did not become ready: {result}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn language_registry_instance(rig: &Rig) -> Value {
    let registry: Value =
        serde_json::from_slice(&fs::read(rig.path.join("ptyd-registry.json")).unwrap()).unwrap();
    let instances: Vec<_> = registry["instances"]
        .as_object()
        .unwrap()
        .values()
        .filter(|instance| instance["spawn"]["kind"] == "pipes_lsp" && instance["closed"] == false)
        .cloned()
        .collect();
    assert_eq!(instances.len(), 1);
    instances[0].clone()
}

#[test]
fn editor_open_rejects_absolute_parent_symlink_escape_and_oversized_files() {
    let rig = Rig::new();
    let repository = rig.path.join("editor-boundary");
    let outside = rig.path.join("outside.rs");
    fs::create_dir_all(&repository).unwrap();
    fs::write(&outside, "secret\n").unwrap();
    symlink(&outside, repository.join("link.rs")).unwrap();
    let large = fs::File::create(repository.join("large.rs")).unwrap();
    large
        .set_len(soyeht_protocol::limits::EDITOR_HISTORY_BYTES as u64 + 1)
        .unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Boundary","path":repository,"idempotencyKey":"boundary-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    for (index, (path, kind)) in [
        (outside.to_str().unwrap(), "invalid_params"),
        ("../outside.rs", "invalid_params"),
        ("link.rs", "invalid_params"),
        ("large.rs", "file_too_large"),
    ]
    .into_iter()
    .enumerate()
    {
        let response = client.request(
            "file.open",
            json!({"workspaceId":workspace,"path":path,"idempotencyKey":format!("refused-{index}")}),
        );
        assert_eq!(response["error"]["data"]["kind"], kind, "{path}");
    }
}

#[test]
fn project_search_is_ignore_aware_symlink_safe_bounded_and_idempotent() {
    let rig = Rig::new();
    let repository = rig.path.join("editor-search");
    let outside = rig.path.join("search-outside.txt");
    fs::create_dir_all(repository.join("src")).unwrap();
    fs::create_dir_all(repository.join("ignored-dir")).unwrap();
    fs::create_dir_all(repository.join(".git")).unwrap();
    fs::write(repository.join(".gitignore"), "ignored.txt\nignored-dir/\n").unwrap();
    fs::write(repository.join("src/main.rs"), "needle\nneedle\n").unwrap();
    fs::write(repository.join("src/many.txt"), "needle\n".repeat(600)).unwrap();
    fs::write(repository.join("ignored.txt"), "needle\n").unwrap();
    fs::write(repository.join("ignored-dir/data.txt"), "needle\n").unwrap();
    fs::write(repository.join(".git/secret"), "needle\n").unwrap();
    fs::write(repository.join("invalid.bin"), [0xff, 0xfe]).unwrap();
    fs::write(&outside, "needle\n").unwrap();
    symlink(&outside, repository.join("src/link.txt")).unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Search","path":repository,"idempotencyKey":"search-workspace"}),
    )["workspace"]["workspaceId"]
        .clone();

    let params = json!({
        "workspaceId":workspace,"query":"needle","caseSensitive":true,
        "limit":500,"idempotencyKey":"search-content"
    });
    let result = client.call("file.search", params.clone());
    assert_eq!(result["items"].as_array().unwrap().len(), 500);
    assert_eq!(result["replaced"], 0);
    assert_eq!(client.call("file.search", params), result);
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| { matches!(item["path"].as_str(), Some("src/main.rs" | "src/many.txt")) })
    );

    let paths = client.call(
        "file.search",
        json!({
            "workspaceId":workspace,"scope":"path","query":"smr",
            "caseSensitive":false,"limit":10,"idempotencyKey":"search-path"
        }),
    );
    assert_eq!(paths["items"][0]["path"], "src/main.rs");
    assert_eq!(paths["items"][0]["preview"], "src/main.rs");
    assert_eq!(paths["items"][0]["range"]["start"]["line"], 0);

    let (snapshot, _) = client.snapshot();
    client.call("events.subscribe", json!({"since":snapshot["cursor"]}));
    let opened = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"src/main.rs","idempotencyKey":"search-open-main"}),
    );
    let file_id = opened["file"]["fileId"].as_str().unwrap().to_owned();
    client.messages.clear();

    let replace_params = (
        "file.search",
        json!({
            "workspaceId":workspace,"scope":"content","query":"needle",
            "replacement":"safe","caseSensitive":true,"limit":10,
            "idempotencyKey":"search-replace"
        }),
    );
    let replaced = client.call(replace_params.0, replace_params.1.clone());
    assert_eq!(replaced["replaced"], 10);
    assert_eq!(replaced["items"].as_array().unwrap().len(), 10);
    assert_eq!(
        client.call(replace_params.0, replace_params.1.clone()),
        replaced
    );
    let mismatch = client.request(
        replace_params.0,
        json!({
            "workspaceId":workspace,"scope":"content","query":"needle",
            "replacement":"different","caseSensitive":true,"limit":10,
            "idempotencyKey":"search-replace"
        }),
    );
    assert_eq!(mismatch["error"]["data"]["kind"], "idempotency_conflict");
    assert_eq!(
        fs::read_to_string(repository.join("src/main.rs")).unwrap(),
        "safe\nsafe\n"
    );
    let read = client.call(
        "file.read",
        json!({"fileId":file_id,"revision":"2","fromLine":0,"lineCount":2}),
    );
    assert_eq!(read["lines"][0]["text"], "safe");
    let history = client.call("file.history", json!({"fileId":file_id,"limit":10}));
    assert_eq!(history["items"].as_array().unwrap().len(), 1);
    assert!(client.messages.iter().any(|frame| {
        let Frame::Json(bytes) = frame else {
            return false;
        };
        let event: Value = serde_json::from_slice(bytes).unwrap();
        event["method"] == "file.saved" && event["params"]["file"]["fileId"] == file_id
    }));
    assert_eq!(
        fs::read_to_string(repository.join("ignored.txt")).unwrap(),
        "needle\n"
    );
    assert_eq!(fs::read_to_string(&outside).unwrap(), "needle\n");
}

#[test]
fn project_replacement_refuses_dirty_files_without_touching_disk() {
    let rig = Rig::new();
    let repository = rig.path.join("editor-dirty-replace");
    fs::create_dir_all(&repository).unwrap();
    let path = repository.join("note.txt");
    fs::write(&path, "clean\n").unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Dirty","path":repository,"idempotencyKey":"dirty-workspace"}),
    )["workspace"]["workspaceId"]
        .clone();
    let opened = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"note.txt","idempotencyKey":"dirty-open"}),
    );
    let autosave_pause = rig.path.join("test-pause-editor-autosave");
    fs::write(&autosave_pause, b"pause").unwrap();
    client.call(
        "file.write",
        json!({
            "fileId":opened["file"]["fileId"],"baseRevision":"1",
            "author":local_file_author(),
            "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":5}},"text":"needle"}],
            "idempotencyKey":"dirty-write"
        }),
    );
    let response = client.request(
        "file.search",
        json!({
            "workspaceId":workspace,"query":"needle","replacement":"safe",
            "caseSensitive":true,"limit":10,"idempotencyKey":"dirty-replace"
        }),
    );
    assert_eq!(response["error"]["data"]["kind"], "stale_version");
    assert_eq!(fs::read_to_string(path).unwrap(), "clean\n");
    fs::remove_file(autosave_pause).unwrap();
}

#[test]
fn project_replacement_recovers_after_disk_write_and_preserves_file_identity() {
    let mut rig = Rig::new();
    let repository = rig.path.join("editor-replace-recovery");
    fs::create_dir_all(&repository).unwrap();
    let path = repository.join("note.txt");
    let before = "needle\n";
    let after = "safe\n";
    fs::write(&path, before).unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Recovery","path":repository,"idempotencyKey":"recovery-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let params = json!({
        "workspaceId":workspace,"query":"needle","replacement":"safe",
        "caseSensitive":true,"limit":10,"idempotencyKey":"recover-replace"
    });
    rig.kill_engine();
    let file_id = stage_interrupted_replacement(&rig, &workspace, &params, before, after);
    fs::write(&path, after).unwrap();
    rig.start();
    let mut restored = rig.client();
    let result = restored.call("file.search", params.clone());
    assert_eq!(result["replaced"], 1);
    assert_eq!(restored.call("file.search", params), result);
    let opened = restored.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"note.txt","idempotencyKey":"open-replaced"}),
    );
    assert_eq!(opened["file"]["fileId"], file_id);
    assert_eq!(opened["file"]["revision"], "1");
    let history = restored.call("file.history", json!({"fileId":file_id,"limit":10}));
    assert_eq!(history["items"].as_array().unwrap().len(), 1);
    assert_eq!(history["items"][0]["author"]["name"], "you");
    rig.kill_engine();
    let store = Store::open(&rig.path).unwrap();
    assert!(store.active_file_replacements().unwrap().is_empty());
}
