use super::*;
use crate::agents::registry::builtin_entries;
use soyeht_ptyd::wire::ProcessKind;
use std::collections::BTreeMap;

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn spawn(items: &[&str]) -> Spawn {
    Spawn {
        instance_key: "adapter".into(),
        generation: 1,
        kind: ProcessKind::PipesAcp,
        cwd: "/".into(),
        argv: argv(items),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    }
}

fn pinned(locked: &Locked) -> String {
    format!("{}@{}", locked.package, locked.version)
}

#[test]
fn only_the_exact_pinned_launch_of_a_locked_adapter_is_installed() {
    let locked = &LOCKED[0];
    let package = pinned(locked);
    let launch = argv(&["npx", "--yes", &package, "--flag"]);
    let (found, rest) = locked_launch(&launch).unwrap();
    assert_eq!(found.name, locked.name);
    assert_eq!(rest, ["--flag"]);
    assert!(locked_launch(&argv(&["/opt/node/bin/npx", "--yes", &package])).is_some());
    for refused in [
        argv(&["npx", &package]),
        argv(&["npx", "--yes", &format!("{}@0.0.1", locked.package)]),
        argv(&["npx", "--yes", locked.package]),
        argv(&["uvx", "--yes", &package]),
        argv(&["node", "--yes", &package]),
        argv(&["npx", "--yes"]),
        argv(&[]),
    ] {
        assert!(locked_launch(&refused).is_none(), "{refused:?}");
    }
}

/// What the catalogue tells a picker before the first start: the package a built-in adapter of a
/// package runner fetches and whether the engine installs it from its lock.
#[test]
fn the_catalogue_names_the_package_of_a_runner_adapter_and_whether_it_is_locked() {
    let entries = builtin_entries().unwrap();
    let package = |id: &str| {
        let entry = entries.iter().find(|entry| entry.launch.id == id).unwrap();
        runner_package(&entry.launch)
    };
    assert_eq!(
        package("registry.claude-acp"),
        Some((
            "@agentclientprotocol/claude-agent-acp@0.100.0".to_owned(),
            true
        ))
    );
    assert_eq!(
        package("registry.codex-acp"),
        Some(("@agentclientprotocol/codex-acp@1.11.0".to_owned(), true))
    );
    // Fetched by the runner, nothing checked.
    assert_eq!(
        package("registry.gemini"),
        Some(("@google/gemini-cli@0.59.0".to_owned(), false))
    );
    assert_eq!(
        package("registry.fast-agent"),
        Some(("fast-agent-acp==0.10.1".to_owned(), false))
    );
    // Not a package runner: nothing to download.
    assert_eq!(package("registry.cursor"), None);
    assert_eq!(package("registry.opencode"), None);
    // A custom entry runs as its author wrote it, whatever its command.
    let mut custom = entries
        .iter()
        .find(|entry| entry.launch.id == "registry.gemini")
        .unwrap()
        .launch
        .clone();
    custom.id = "custom.gemini".into();
    assert_eq!(runner_package(&custom), None);
}

/// Each lock is the lock of the package and version the registry snapshot launches, and is the
/// kind the engine installs with scripts off: a snapshot that moves on without its lock fails here
/// instead of going back to `npx`.
#[test]
fn every_lock_matches_the_registry_snapshot_and_pins_every_package_by_integrity() {
    let entries = builtin_entries().unwrap();
    for locked in &LOCKED {
        let launches = entries.iter().filter(|entry| {
            entry.launch.command == "npx"
                && entry.launch.args == ["--yes".to_owned(), pinned(locked)]
        });
        assert_eq!(launches.count(), 1, "{}", locked.package);
        let manifest: serde_json::Value = serde_json::from_str(locked.manifest).unwrap();
        assert_eq!(
            manifest["dependencies"],
            serde_json::json!({ locked.package: locked.version })
        );
        let lock: serde_json::Value = serde_json::from_str(locked.lock).unwrap();
        assert_eq!(lock["lockfileVersion"], 3);
        assert_eq!(
            lock["packages"][""]["dependencies"],
            manifest["dependencies"]
        );
        let own = &lock["packages"][format!("node_modules/{}", locked.package)];
        assert_eq!(own["version"], locked.version);
        let script = locked.entry.strip_prefix(locked.package).unwrap();
        assert!(
            own["bin"]
                .as_object()
                .unwrap()
                .values()
                .any(|bin| format!("/{}", bin.as_str().unwrap()) == script),
            "the entry is not the package's bin"
        );
        for (path, package) in lock["packages"].as_object().unwrap() {
            if path.is_empty() {
                continue;
            }
            assert!(
                package["integrity"]
                    .as_str()
                    .is_some_and(|hash| hash.starts_with("sha") && hash.len() > 80),
                "{path} has no strong integrity hash"
            );
            assert!(
                package["resolved"]
                    .as_str()
                    .is_some_and(|url| url.starts_with("https://")),
                "{path}"
            );
            // Scripts are off for these installs: a package that needs one is a decision to take.
            assert!(package["hasInstallScript"].is_null(), "{path} has a script");
        }
    }
}

#[cfg(unix)]
mod installing {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, path::PathBuf};

    /// A stand-in for `npm`: it records its arguments and the folder it ran in, and lays out the
    /// tree of `locked` with `version` unless told to fail.
    struct Npm {
        program: PathBuf,
        log: PathBuf,
    }

    impl Npm {
        fn new(profile: &Path, locked: &Locked, version: &str, succeed: bool) -> Self {
            let program = profile.join("fake-npm");
            let log = profile.join("npm-calls");
            let package = locked.package;
            let script = format!(
                "#!/bin/sh\necho \"$PWD $*\" >> '{log}'\n{body}\n",
                log = log.display(),
                body = if succeed {
                    format!(
                        "mkdir -p node_modules/{package}/dist\n\
                         echo '{{\"version\":\"{version}\"}}' > node_modules/{package}/package.json\n\
                         echo '' > node_modules/{package}/dist/index.js"
                    )
                } else {
                    "exit 1".to_owned()
                }
            );
            fs::write(&program, script).unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            Self { program, log }
        }

        fn calls(&self) -> Vec<String> {
            fs::read_to_string(&self.log)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        }

        fn program(&self) -> &str {
            self.program.to_str().unwrap()
        }
    }

    fn launch(locked: &Locked) -> Spawn {
        spawn(&["npx", "--yes", &pinned(locked), "--acp"])
    }

    #[test]
    fn the_first_start_installs_from_the_lock_and_later_starts_run_the_same_tree() {
        let profile = soyeht_platform::testing::scratch_dir("adapter-install");
        let locked = &LOCKED[0];
        let npm = Npm::new(&profile, locked, locked.version, true);

        let first = installed_spawn_with(&profile, launch(locked), npm.program()).unwrap();
        let entry = locked.entry_path(&locked.folder(&profile));
        assert_eq!(first.argv, ["node", entry.to_str().unwrap(), "--acp"]);
        assert!(entry.is_file());
        let calls = npm.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].ends_with("ci --ignore-scripts --no-audit --no-fund --loglevel=error"));
        // It ran in a folder that held only the engine's manifest and lock: nothing of a
        // repository's, and no configuration of its own.
        let folder = calls[0].split(' ').next().unwrap();
        assert!(folder.contains("installed"), "{folder}");
        let staging = locked.folder(&profile);
        assert_eq!(
            fs::read_to_string(staging.join("package-lock.json")).unwrap(),
            locked.lock
        );

        // The next start, and the one of a harness switch, find it and run no npm.
        let second = installed_spawn_with(&profile, launch(locked), npm.program()).unwrap();
        assert_eq!(second.argv, first.argv);
        assert_eq!(npm.calls().len(), 1);
        // The folder holds nothing but the tree.
        let leftovers: Vec<_> = fs::read_dir(staging.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "{leftovers:?}");
        fs::remove_dir_all(profile).unwrap();
    }

    #[test]
    fn a_tree_that_is_not_made_from_this_lock_is_made_again() {
        let profile = soyeht_platform::testing::scratch_dir("adapter-reinstall");
        let locked = &LOCKED[1];
        let npm = Npm::new(&profile, locked, locked.version, true);
        installed_spawn_with(&profile, launch(locked), npm.program()).unwrap();
        fs::write(locked.folder(&profile).join(MARKER), "another lock").unwrap();
        installed_spawn_with(&profile, launch(locked), npm.program()).unwrap();
        assert_eq!(npm.calls().len(), 2);
        assert!(locked.installed(&locked.folder(&profile)));
        fs::remove_dir_all(profile).unwrap();
    }

    #[test]
    fn a_failed_install_starts_nothing_and_leaves_nothing_behind() {
        let profile = soyeht_platform::testing::scratch_dir("adapter-failed");
        let locked = &LOCKED[0];
        for (version, succeed) in [(locked.version, false), ("9.9.9", true)] {
            let npm = Npm::new(&profile, locked, version, succeed);
            let error = installed_spawn_with(&profile, launch(locked), npm.program()).unwrap_err();
            assert_eq!(
                errors::kind(&error),
                soyeht_protocol::ErrorKind::HarnessUnavailable
            );
            assert!(!locked.folder(&profile).exists());
            let installed = locked.folder(&profile).parent().unwrap().to_owned();
            assert!(
                fs::read_dir(installed).unwrap().next().is_none(),
                "a staging folder was left"
            );
        }
        fs::remove_dir_all(profile).unwrap();
    }

    #[test]
    fn any_other_process_is_left_as_it_is_and_runs_no_npm() {
        let profile = soyeht_platform::testing::scratch_dir("adapter-other");
        let npm = Npm::new(&profile, &LOCKED[0], LOCKED[0].version, true);
        for items in [
            vec!["npx", "--yes", "some-other@1.0.0"],
            vec!["/bin/zsh", "-l"],
            vec!["uvx", "fast-agent-acp==0.10.1", "-x"],
        ] {
            let other = spawn(&items);
            let kept = installed_spawn_with(&profile, other.clone(), npm.program()).unwrap();
            assert_eq!(kept.argv, other.argv);
        }
        assert!(npm.calls().is_empty());
        fs::remove_dir_all(profile).unwrap();
    }
}
