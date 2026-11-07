//! Workspace trust (ADR 0150, contract 18): a folder the person has not trusted never runs its
//! own files through a language server. The fake server writes what it was started with into
//! the folder, so these tests read the arguments, the environment and the options it got.
use super::language_and_project_search::decide_folder;
use super::*;

const RECORD: &str = ".soyeht-fake-lsp-record.json";

fn status(client: &mut Client, workspace: &str, language: &str) -> Option<Value> {
    client.call(
        "lsp.status",
        json!({"workspaceId":workspace,"language":language}),
    )["servers"][0]
        .as_object()
        .map(|server| Value::Object(server.clone()))
}

fn wait_state(client: &mut Client, workspace: &str, language: &str, state: &str) -> Value {
    eventually("the language server state", || {
        match status(client, workspace, language) {
            Some(server) if server["state"] == state => Ok(server),
            other => Err(format!("{other:?}")),
        }
    })
}

/// What the fake server wrote once it had been initialized.
fn record(folder: &Path) -> Value {
    eventually("the fake server's record", || {
        let bytes = fs::read(folder.join(RECORD)).map_err(|error| error.to_string())?;
        let record: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        (record["initialized"] == true)
            .then_some(record)
            .ok_or_else(|| "not initialized".to_owned())
    })
}

fn live_servers(rig: &Rig) -> usize {
    let registry: Value =
        serde_json::from_slice(&fs::read(rig.path.join("ptyd-registry.json")).unwrap()).unwrap();
    registry["instances"]
        .as_object()
        .unwrap()
        .values()
        .filter(|instance| instance["spawn"]["kind"] == "pipes_lsp" && instance["closed"] == false)
        .count()
}

fn trust_of(client: &mut Client, workspace: &str) -> String {
    client
        .section("trust")
        .into_iter()
        .find(|entry| entry["workspaceId"] == workspace)
        .map(|entry| entry["trust"].as_str().unwrap().to_owned())
        .expect("a trust entry for the workspace")
}

fn folder_with(rig: &Rig, name: &str, file: &str) -> PathBuf {
    let folder = rig.path.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join(file), "x\n").unwrap();
    folder
}

fn open_in(client: &mut Client, folder: &Path, name: &str, file: &str) -> String {
    let workspace = create_workspace(client, name, folder, &format!("{name}-workspace"));
    client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":file,"idempotencyKey":format!("{name}-open")}),
    );
    workspace
}

#[test]
fn a_folder_nobody_trusted_never_starts_rust_analyzer_and_says_why() {
    let rig = Rig::new();
    let folder = folder_with(&rig, "restricted-rust", "main.rs");
    let mut client = rig.client();
    let workspace = open_in(&mut client, &folder, "Restricted rust", "main.rs");

    let server = wait_state(&mut client, &workspace, "rust", "restricted");
    assert!(
        server["detail"]
            .as_str()
            .unwrap()
            .contains("Trust the folder"),
        "{server}"
    );
    assert!(server.get("safeMode").is_none());
    assert_eq!(trust_of(&mut client, &workspace), "undecided");
    // Several ticks later still no process was started, and the folder was never run.
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(25));
        assert_eq!(live_servers(&rig), 0);
    }
    assert!(!folder.join(RECORD).exists());
    // A request for the server answers at once instead of waiting for it.
    let file_id = client.section("files")[0]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let refused = client.request("file.symbols", json!({"fileId":file_id,"revision":"1"}));
    assert_eq!(refused["error"]["data"]["kind"], "harness_unavailable");
}

#[test]
fn trusting_a_folder_restarts_the_server_with_every_feature_and_restricting_stops_it() {
    let rig = Rig::new();
    let folder = folder_with(&rig, "trusted-rust", "main.rs");
    let mut client = rig.client();
    let workspace = open_in(&mut client, &folder, "Trusted rust", "main.rs");
    wait_state(&mut client, &workspace, "rust", "restricted");

    let decided = decide_folder(&rig, &workspace, "trusted");
    assert_eq!(decided["trust"], "trusted");
    assert_eq!(decided["workspaceIds"], json!([workspace]));
    wait_state(&mut client, &workspace, "rust", "ready");
    let started = record(&folder);
    assert!(
        started["argv"][0]
            .as_str()
            .unwrap()
            .ends_with("rust-analyzer")
    );
    assert!(started["initializationOptions"].is_null());
    assert_eq!(trust_of(&mut client, &workspace), "trusted");
    assert_eq!(
        rig.database_probe()
            .event_count("project.trust_changed")
            .unwrap(),
        1
    );

    decide_folder(&rig, &workspace, "restricted");
    wait_state(&mut client, &workspace, "rust", "restricted");
    eventually("the server's process to be closed", || {
        (live_servers(&rig) == 0)
            .then_some(())
            .ok_or_else(|| "still running".to_owned())
    });
    assert_eq!(trust_of(&mut client, &workspace), "restricted");
}

#[test]
fn the_trust_of_a_folder_survives_an_engine_restart() {
    let mut rig = Rig::new();
    let folder = folder_with(&rig, "durable-rust", "main.rs");
    let mut client = rig.client();
    let workspace = open_in(&mut client, &folder, "Durable rust", "main.rs");
    let other = folder_with(&rig, "durable-other", "main.rs");
    let other_workspace = create_workspace(&mut client, "Other", &other, "durable-other-workspace");
    decide_folder(&rig, &workspace, "trusted");
    wait_state(&mut client, &workspace, "rust", "ready");
    drop(client);

    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    assert_eq!(trust_of(&mut restored, &workspace), "trusted");
    assert_eq!(trust_of(&mut restored, &other_workspace), "undecided");
    wait_state(&mut restored, &workspace, "rust", "ready");
    // Another workspace over the same folder shares the answer, as the event says.
    let again = create_workspace(&mut restored, "Again", &folder, "durable-again-workspace");
    assert_eq!(trust_of(&mut restored, &again), "trusted");
}

#[test]
fn python_runs_in_its_safe_mode_in_a_restricted_folder_and_trust_lifts_it() {
    let rig = Rig::new();
    let folder = folder_with(&rig, "restricted-python", "main.py");
    let mut client = rig.client();
    let workspace = open_in(&mut client, &folder, "Restricted python", "main.py");

    let server = wait_state(&mut client, &workspace, "python", "ready");
    if cfg!(windows) {
        return;
    }
    assert_eq!(server["safeMode"], true);
    let started = record(&folder);
    assert_eq!(started["argv"].as_array().unwrap().len(), 2);
    assert_eq!(started["argv"][1], "--stdio");
    let environment = started["env"].as_object().unwrap();
    assert!(
        environment.keys().all(|name| !name.starts_with("CARGO")
            && !name.starts_with("RUSTC")
            && name != "PYTHONPATH"),
        "{environment:?}"
    );

    decide_folder(&rig, &workspace, "trusted");
    let server = eventually("the trusted server", || {
        match status(&mut client, &workspace, "python") {
            Some(server) if server["state"] == "ready" && server["generation"] == 2 => Ok(server),
            other => Err(format!("{other:?}")),
        }
    });
    assert!(server.get("safeMode").is_none());
}

#[cfg(unix)]
#[test]
fn typescript_gets_the_installed_tsserver_and_no_type_installs_until_the_folder_is_trusted() {
    let rig = Rig::new();
    let folder = folder_with(&rig, "restricted-typescript", "main.ts");
    // The folder's own TypeScript, which an unrestricted server would run.
    fs::create_dir_all(folder.join("node_modules/typescript/lib")).unwrap();
    fs::write(folder.join("node_modules/typescript/lib/tsserver.js"), "").unwrap();
    let mut client = rig.client();
    let workspace = open_in(&mut client, &folder, "Restricted typescript", "main.ts");

    let server = wait_state(&mut client, &workspace, "typescript", "ready");
    assert_eq!(server["safeMode"], true);
    let options = record(&folder)["initializationOptions"].clone();
    let installed = soyeht_platform::fs::canonicalize(
        &rig.path.join("npm/node_modules/typescript/lib/tsserver.js"),
    )
    .unwrap();
    assert_eq!(options["tsserver"]["path"], installed.to_str().unwrap());
    assert_eq!(options["disableAutomaticTypingAcquisition"], true);
    assert_eq!(options["plugins"], json!([]));

    decide_folder(&rig, &workspace, "trusted");
    eventually("the trusted server", || {
        match status(&mut client, &workspace, "typescript") {
            Some(server) if server["state"] == "ready" && server["generation"] == 2 => Ok(()),
            other => Err(format!("{other:?}")),
        }
    });
    eventually("the options of the trusted server", || {
        let record = record(&folder);
        record["initializationOptions"]
            .is_null()
            .then_some(())
            .ok_or_else(|| record.to_string())
    });
}

#[test]
fn swift_does_not_start_in_a_restricted_folder() {
    let rig = Rig::new();
    let folder = folder_with(&rig, "restricted-swift", "main.swift");
    let mut client = rig.client();
    let workspace = open_in(&mut client, &folder, "Restricted swift", "main.swift");
    let server = wait_state(&mut client, &workspace, "swift", "restricted");
    assert!(server["detail"].as_str().unwrap().contains("sourcekit-lsp"));
    assert_eq!(live_servers(&rig), 0);
}

#[test]
fn an_agent_and_an_unattested_program_cannot_trust_a_folder() {
    let rig = Rig::new();
    let folder = folder_with(&rig, "refused-trust", "main.rs");
    let mut human = rig.client();
    let workspace = open_in(&mut human, &folder, "Refused trust", "main.rs");
    let (agent, pane) = rig.agent_in_workspace(&workspace);
    drop(agent);
    let binding = rig.database_probe().agent_binding(&pane).unwrap();
    let (mut executor, welcome) = binding_hello(&rig.path.join("engine.sock"), &binding);
    assert_eq!(welcome["result"]["role"], "agent");

    let params =
        |key: &str| json!({"workspaceId":workspace,"trust":"trusted","idempotencyKey":key});
    let refused = binding_request(
        &mut executor,
        10,
        "project.trust.set",
        params("agent-trust"),
    );
    assert_eq!(refused["error"]["data"]["kind"], "forbidden_role");
    if soyeht_engine::ui_attestation::available() {
        // A shell on the local socket is not the signed app.
        let shell = human.request("project.trust.set", params("shell-trust"));
        assert_eq!(shell["error"]["data"]["kind"], "forbidden_role");
    }
    assert_eq!(trust_of(&mut human, &workspace), "undecided");
    assert_eq!(
        rig.database_probe()
            .event_count("project.trust_changed")
            .unwrap(),
        0
    );
    wait_state(&mut human, &workspace, "rust", "restricted");
}
