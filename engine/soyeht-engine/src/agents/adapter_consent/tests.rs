use super::*;
use crate::{outbox, supervisor_link::tests::core_with_unstarted_pane};
use serde_json::json;
use soyeht_protocol::{Actor, ErrorKind};
use soyeht_ptyd::wire::{ProcessKind, Spawn};
use soyeht_store::database::Store;
use std::collections::BTreeMap;

const GEMINI: &str = "@google/gemini-cli@0.59.0";

fn launch(id: &str) -> HarnessLaunch {
    registry::builtin_entries()
        .unwrap()
        .into_iter()
        .find(|entry| entry.launch.id == id)
        .unwrap()
        .launch
}

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// A store in a folder of its own, which the caller removes.
fn store() -> (Store, std::path::PathBuf) {
    let directory = soyeht_platform::testing::scratch_dir("adapter-consent");
    (Store::open(&directory).unwrap(), directory)
}

fn acknowledge(store: &mut Store, package: &str) {
    store
        .mutate("test", package, package, |change| {
            soyeht_store::adapter_acknowledgement::acknowledge(change, package, 1)?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();
}

fn kind(result: Result<()>) -> Option<ErrorKind> {
    result.err().map(|error| errors::kind(&error))
}

#[test]
fn only_a_builtin_package_the_engine_does_not_lock_needs_an_acknowledgement() {
    assert_eq!(
        unchecked_package(&launch("registry.gemini")).as_deref(),
        Some(GEMINI)
    );
    assert_eq!(
        unchecked_package(&launch("registry.fast-agent")).as_deref(),
        Some("fast-agent-acp==0.10.1")
    );
    // Installed from the engine's lock, or no package at all.
    for id in [
        "registry.claude-acp",
        "registry.codex-acp",
        "registry.cursor",
        "registry.opencode",
    ] {
        assert_eq!(unchecked_package(&launch(id)), None, "{id}");
    }
    // A custom entry runs as its author wrote it, whatever its command.
    let mut custom = launch("registry.gemini");
    custom.id = "custom.gemini".into();
    assert_eq!(unchecked_package(&custom), None);
}

/// The check where a process is sent reads its command line, the catalogue reads the launch it
/// came from. They must say the same for every entry of the snapshot, so that a form the check
/// does not read (`-y`, `--package`) fails here and not silently at launch.
#[test]
fn the_check_at_launch_agrees_with_the_catalogue_for_every_snapshot_entry() {
    let mut runners = 0;
    for entry in registry::builtin_entries().unwrap() {
        let launch = entry.launch;
        let command_line: Vec<String> = std::iter::once(launch.command.clone())
            .chain(launch.args.iter().cloned())
            .collect();
        assert_eq!(
            unchecked_package_of(&command_line).map(str::to_owned),
            unchecked_package(&launch),
            "{}",
            launch.id
        );
        runners += usize::from(adapter_install::runner_package(&launch).is_some());
    }
    // Entries that run a package exist, so the comparison above is not empty.
    assert!(runners > 10, "{runners}");
}

#[test]
fn a_start_is_refused_until_its_exact_package_is_acknowledged() {
    let (mut store, directory) = store();
    let gemini = launch("registry.gemini");
    assert_eq!(
        kind(require(&store, &gemini, None)),
        Some(ErrorKind::AdapterNotAcknowledged)
    );
    // The request that acknowledges counts for its own start; another version does not.
    assert_eq!(kind(require(&store, &gemini, Some(GEMINI))), None);
    assert_eq!(
        kind(require(&store, &gemini, Some("@google/gemini-cli@0.58.0"))),
        Some(ErrorKind::AdapterNotAcknowledged)
    );
    // A stored acknowledgement is of one version: the next one asks again.
    acknowledge(&mut store, "@google/gemini-cli@0.58.0");
    assert!(require(&store, &gemini, None).is_err());
    acknowledge(&mut store, GEMINI);
    assert_eq!(kind(require(&store, &gemini, None)), None);
    // Nothing to acknowledge for the adapters the engine locks.
    let claude = launch("registry.claude-acp");
    assert_eq!(kind(require(&store, &claude, None)), None);
    drop(store);
    crate::test_paths::remove_after((), directory);
}

#[test]
fn the_process_about_to_be_sent_is_checked_by_its_command_line() {
    let (mut store, directory) = store();
    for refused in [
        argv(&["/opt/homebrew/bin/npx", "--yes", GEMINI, "--acp"]),
        argv(&["npx", "--yes", GEMINI]),
        argv(&["uvx", "fast-agent-acp==0.10.1", "-x"]),
    ] {
        assert_eq!(
            kind(require_spawn(&store, &refused)),
            Some(ErrorKind::AdapterNotAcknowledged),
            "{refused:?}"
        );
    }
    for allowed in [
        // A custom entry that runs another package, a shell, a locked adapter.
        argv(&["npx", "--yes", "some-other@1.0.0"]),
        argv(&["/bin/zsh", "-l"]),
        argv(&[
            "npx",
            "--yes",
            "@agentclientprotocol/claude-agent-acp@0.100.0",
        ]),
        argv(&["npx", GEMINI]),
        argv(&[]),
    ] {
        assert_eq!(kind(require_spawn(&store, &allowed)), None, "{allowed:?}");
    }
    acknowledge(&mut store, GEMINI);
    assert_eq!(
        kind(require_spawn(&store, &argv(&["npx", "--yes", GEMINI]))),
        None
    );
    drop(store);
    crate::test_paths::remove_after((), directory);
}

#[test]
fn the_launch_of_a_process_is_refused_for_a_package_nobody_acknowledged() {
    let (mut core, directory, _pane, _calls) = core_with_unstarted_pane(Vec::new());
    let spawn = Spawn {
        instance_key: "adapter".into(),
        generation: 1,
        kind: ProcessKind::PipesAcp,
        cwd: "/".into(),
        argv: argv(&["npx", "--yes", GEMINI, "--acp"]),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    };
    let refused = core.launch(spawn.clone(), &[]).err().unwrap();
    assert_eq!(errors::kind(&refused), ErrorKind::AdapterNotAcknowledged);

    crate::ports::StorePort::mutate(&mut core.store, "test", "ack", "ack", |change| {
        soyeht_store::adapter_acknowledgement::acknowledge(change, GEMINI, 1)?;
        Ok((json!({}), Vec::new()))
    })
    .unwrap();
    assert!(core.launch(spawn, &[]).is_ok());
    crate::test_paths::remove_after(core, directory);
}

#[test]
fn only_a_person_acknowledges_and_only_the_package_the_agent_runs() {
    let (mut core, directory, _pane, _calls) = core_with_unstarted_pane(Vec::new());
    let (sender, _receiver) = outbox::channel();
    let connection = core.connect(true, false, sender).unwrap();
    let actor = |role| Actor {
        role,
        client: "test".into(),
        device_id: None,
        pane_id: None,
    };
    let gemini = launch("registry.gemini");
    let carrying = |package: &str| Some(package.to_owned());

    core.clients.get_mut(&connection).unwrap().actor = Some(actor(Role::HumanDevice));
    assert_eq!(
        core.acknowledgement_of(&connection, carrying(GEMINI).as_deref(), &gemini)
            .unwrap()
            .as_deref(),
        Some(GEMINI)
    );
    assert_eq!(
        core.acknowledgement_of(&connection, None, &gemini).unwrap(),
        None
    );
    let stale = core.acknowledgement_of(
        &connection,
        carrying("@google/gemini-cli@0.1.0").as_deref(),
        &gemini,
    );
    assert_eq!(errors::kind(&stale.unwrap_err()), ErrorKind::InvalidParams);
    // Nothing to acknowledge for a locked adapter: the field is ignored.
    assert_eq!(
        core.acknowledgement_of(
            &connection,
            carrying(GEMINI).as_deref(),
            &launch("registry.claude-acp")
        )
        .unwrap(),
        None
    );

    // An agent's binding is refused, whatever it sends.
    for role in [Role::Agent, Role::Chief] {
        core.clients.get_mut(&connection).unwrap().actor = Some(actor(role));
        let refused = core.acknowledgement_of(&connection, carrying(GEMINI).as_deref(), &gemini);
        assert_eq!(
            errors::kind(&refused.unwrap_err()),
            ErrorKind::ForbiddenRole
        );
    }
    crate::test_paths::remove_after(core, directory);
}
