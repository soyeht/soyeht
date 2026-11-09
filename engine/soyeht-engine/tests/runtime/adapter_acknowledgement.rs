//! An adapter that its package runner downloads unchecked starts only after a person
//! acknowledged that exact package (ADR 0143).

use super::*;

const PACKAGE: &str = "@google/gemini-cli@0.59.0";

/// An `npx` that starts the fake agent, as the runner of the registry's unchecked adapters would.
#[cfg(unix)]
fn install_runner(rig: &Rig) {
    write_script(
        &rig.path.join("bin/npx"),
        &format!(
            "#!/bin/sh\nexec '{}' --state-dir '{}'\n",
            sibling_binary("soyeht-fake-acp").display(),
            rig.path.join("gemini-native").display()
        ),
    );
}

fn kind(response: &Value) -> &Value {
    &response["error"]["data"]["kind"]
}

fn gemini(client: &mut Client) -> Value {
    let list = client.call("harness.list", json!({"limit":64,"rescan":true}));
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "registry.gemini")
        .unwrap()
        .clone()
}

fn create(client: &mut Client, workspace: &str, key: &str, extra: Value) -> Value {
    let mut params = json!({"workspaceId":workspace,"mode":"agent","harness":"registry.gemini",
        "idempotencyKey":key});
    for (name, value) in extra.as_object().unwrap() {
        params[name] = value.clone();
    }
    client.request("pane.create", params)
}

#[cfg(unix)]
#[test]
fn an_unchecked_package_starts_only_for_a_person_who_acknowledged_it_and_only_that_version() {
    let rig = Rig::new();
    install_runner(&rig);
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Repository", &rig.path, "ack-workspace");

    // The catalogue says what the first start does, and keeps saying it.
    let item = gemini(&mut client);
    assert_eq!(item["package"], PACKAGE);
    assert_eq!(item["packageLocked"], false);
    assert_eq!(item["setupReason"], "registry_package_unverified");

    // Without the acknowledgement, and with another version's, nothing starts and no pane is made.
    let panes = |client: &mut Client| {
        client
            .snapshot()
            .1
            .iter()
            .filter(|part| part["section"] == "panes")
            .map(|part| part["data"].as_array().map_or(0, Vec::len))
            .sum::<usize>()
    };
    let before = panes(&mut client);
    let refused = create(&mut client, &workspace, "ack-none", json!({}));
    assert_eq!(kind(&refused), "adapter_not_acknowledged", "{refused}");
    let stale = create(
        &mut client,
        &workspace,
        "ack-stale",
        json!({"acknowledgedPackage":"@google/gemini-cli@0.1.0"}),
    );
    assert_eq!(kind(&stale), "invalid_params", "{stale}");
    assert_eq!(panes(&mut client), before);
    assert_eq!(
        gemini(&mut client)["setupReason"],
        "registry_package_unverified"
    );

    // A person's request that carries the package starts it, and stores the acknowledgement.
    let started = create(
        &mut client,
        &workspace,
        "ack-given",
        json!({"acknowledgedPackage":PACKAGE}),
    );
    assert!(started.get("error").is_none(), "{started}");
    let pane = started["result"]["paneId"].as_str().unwrap().to_owned();
    wait_until("the acknowledged adapter did not start", || {
        client.pane(&pane)["agent"]["sessionId"].is_string()
    });

    // The next start of the same package needs no new acknowledgement (a restart, a task), and the
    // notice is gone.
    let again = create(&mut client, &workspace, "ack-again", json!({}));
    assert!(again.get("error").is_none(), "{again}");
    assert_ne!(
        gemini(&mut client)["setupReason"],
        "registry_package_unverified"
    );
}

#[cfg(unix)]
#[test]
fn a_switch_to_an_unchecked_package_needs_the_acknowledgement_too() {
    let rig = Rig::new();
    install_runner(&rig);
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Test","path":rig.path,"idempotencyKey":"switch-ack-workspace"}),
    )["workspace"]["workspaceId"]
        .clone();
    rig.install_fake_harness("config");
    let (mut client, pane) = rig.open_installed_agent(client, &workspace);

    let refused = client.request(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"registry.gemini","idempotencyKey":"switch-none"}),
    );
    assert_eq!(kind(&refused), "adapter_not_acknowledged", "{refused}");
    // Nothing was locked for a switch that did not begin.
    assert!(client.pane(&pane)["agent"]["harnessSwitch"].is_null());

    let given = client.request(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"registry.gemini","acknowledgedPackage":PACKAGE,
            "idempotencyKey":"switch-given"}),
    );
    assert!(given.get("error").is_none(), "{given}");
}
