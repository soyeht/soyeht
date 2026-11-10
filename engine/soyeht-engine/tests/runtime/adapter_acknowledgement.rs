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

/// Whether `harness.list` offers the unchecked package's adapter. It never does (ADR 0143): the
/// catalogue lists only the supported harnesses, and a request reaches this one by its id.
fn listed(client: &mut Client) -> bool {
    let list = client.call("harness.list", json!({"limit":64,"rescan":true}));
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == "registry.gemini")
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

    // The catalogue does not offer it, so the acknowledgement below is reached by explicit id, as a
    // person who knows the id (or the command line) would. The notice that a picker would show for
    // an unlisted entry is covered by the unit tests of `agents::catalog`.
    assert!(!listed(&mut client));

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

    // The next start of the same package needs no new acknowledgement (a restart, a task). Being
    // acknowledged does not list it either.
    let again = create(&mut client, &workspace, "ack-again", json!({}));
    assert!(again.get("error").is_none(), "{again}");
    assert!(!listed(&mut client));
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
